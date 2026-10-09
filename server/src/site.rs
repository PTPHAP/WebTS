use crate::{
    app::{Api, App, Error},
    settings,
};
use axum::{Json, extract::State, http::HeaderMap};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use std::{io::Cursor, sync::Arc};
use zeroize::Zeroizing;

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Home {
    pub title: String,
    pub subtitle: String,
    pub image: String,
    pub announcements: Vec<Announcement>,
}
impl Default for Home {
    fn default() -> Self {
        Self {
            title: "让声音，跨越距离。".into(),
            subtitle: "与你熟悉的人，在熟悉的频道相聚。打开浏览器，让 TeamSpeak 随时在身边。"
                .into(),
            image: String::new(),
            announcements: Vec::new(),
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Announcement {
    pub id: String,
    pub title: String,
    pub text: String,
    pub image: String,
    pub enabled: bool,
}
fn text(value: &str, max: usize, multiline: bool) -> bool {
    value.chars().count() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && !(multiline && c == '\n'))
}
fn image(value: &mut String) -> Api<()> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > 175000 {
        return Err(Error::bad("首页图片最大128KiB"));
    }
    let bytes = STANDARD
        .decode(
            value
                .strip_prefix("data:image/jpeg;base64,")
                .ok_or_else(|| Error::bad("首页图片仅接受JPEG"))?,
        )
        .map_err(|_| Error::bad("图片编码无效"))?;
    if bytes.len() > 128 * 1024 {
        return Err(Error::bad("首页图片最大128KiB"));
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Jpeg);
    let mut limits = Limits::default();
    limits.max_image_width = Some(1600);
    limits.max_image_height = Some(1200);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|_| Error::bad("图片无效或尺寸过大"))?;
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 75)
        .encode_image(&decoded)
        .map_err(|_| Error::bad("图片处理失败"))?;
    if out.get_ref().len() > 128 * 1024 {
        return Err(Error::bad("图片过于复杂，请缩小后重试"));
    }
    *value = format!(
        "data:image/jpeg;base64,{}",
        STANDARD.encode(out.into_inner())
    );
    Ok(())
}
impl Home {
    fn normalize(&mut self) -> Api<()> {
        if self.title.trim().is_empty()
            || !text(&self.title, 80, false)
            || !text(&self.subtitle, 300, true)
            || self.announcements.len() > 6
        {
            return Err(Error::bad("首页标题、介绍或公告数量无效"));
        }
        image(&mut self.image)?;
        let mut ids = std::collections::HashSet::new();
        for row in &mut self.announcements {
            if row.id.is_empty()
                || row.id.len() > 64
                || !row
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                || !ids.insert(row.id.clone())
                || row.title.trim().is_empty()
                || !text(&row.title, 100, false)
                || !text(&row.text, 2000, true)
            {
                return Err(Error::bad("公告内容无效"));
            }
            image(&mut row.image)?;
        }
        if self.image.len()
            + self
                .announcements
                .iter()
                .map(|r| r.image.len())
                .sum::<usize>()
            > 700000
        {
            return Err(Error::bad("首页图片总量过大，请减少图片"));
        }
        Ok(())
    }
}
pub async fn public(State(app): State<Arc<App>>) -> Json<Home> {
    let mut home = app.runtime.read().unwrap().settings.home.clone();
    home.announcements.retain(|r| r.enabled);
    Json(home)
}
pub async fn get(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Home>> {
    settings::admin(&app, &headers)?;
    Ok(Json(app.runtime.read().unwrap().settings.home.clone()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    password: String,
    home: Home,
}
pub async fn save(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Update>,
) -> Api<Json<Home>> {
    app.work(move |a| {
        settings::admin(a, &headers)?;
        let password = Zeroizing::new(body.password);
        let session = a.reauthenticate(&headers, &password)?;
        body.home.normalize()?;
        let mut current = a.runtime.write().unwrap();
        let mut updated = current.settings.clone();
        updated.home = body.home;
        let plaintext = Zeroizing::new(serde_json::to_vec(&updated).map_err(anyhow::Error::from)?);
        let ciphertext = a.vault.seal(0, "site-settings", "v1", &plaintext)?;
        a.db.save_settings(session.user.id, &session.hash, &ciphertext)
            .map_err(|_| Error::bad("管理员登录已失效"))?;
        current.settings.home = updated.home;
        Ok(Json(current.settings.home.clone()))
    })
    .await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn homepage_rejects_scripts_as_images_and_oversize_content() {
        let mut home = Home {
            image: "data:image/svg+xml;base64,PHN2Zy8+".into(),
            ..Default::default()
        };
        assert!(home.normalize().is_err());
        home.image = format!(
            "data:image/jpeg;base64,{}",
            STANDARD.encode(b"<script>alert(1)</script>")
        );
        assert!(home.normalize().is_err());
        home.image.clear();
        home.title = "a".repeat(81);
        assert!(home.normalize().is_err());
        home.title = "<script>plain text only</script>".into();
        assert!(home.normalize().is_ok());
    }
}
