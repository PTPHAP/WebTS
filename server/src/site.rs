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
    pub site_name: String,
    pub site_icon: String,
    pub footer_html: String,
    pub operator: String,
    pub contact: String,
    pub data_details: String,
    pub privacy_policy: String,
    pub terms: String,
    pub title: String,
    pub subtitle: String,
    pub image: String,
    pub announcements: Vec<Announcement>,
}
impl Default for Home {
    fn default() -> Self {
        Self {
            site_name: "WebTS".into(),
            site_icon: String::new(),
            footer_html: String::new(),
            operator: String::new(),
            contact: String::new(),
            data_details: String::new(),
            privacy_policy: include_str!("../../PRIVACY.md").replace("\r\n", "\n"),
            terms: include_str!("../../TERMS.md").replace("\r\n", "\n"),
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
        if self.site_name.trim().is_empty()
            || !text(&self.site_name, 40, false)
            || !text(&self.operator, 100, false)
            || !text(&self.contact, 200, false)
            || !text(&self.data_details, 4000, true)
            || self.footer_html.len() > 16384
            || self.privacy_policy.len() > 65536
            || self.terms.len() > 65536
            || self.privacy_policy.trim().is_empty()
            || self.terms.trim().is_empty()
            || !text(&self.privacy_policy, 65536, true)
            || !text(&self.terms, 65536, true)
        {
            return Err(Error::bad("站点名称、协议或页脚超出限制"));
        }
        self.footer_html = clean_footer(&self.footer_html);
        if !self.site_icon.is_empty() {
            let bytes = STANDARD
                .decode(
                    self.site_icon
                        .strip_prefix("data:image/png;base64,")
                        .ok_or_else(|| Error::bad("站点图标仅接受PNG"))?,
                )
                .map_err(|_| Error::bad("图标编码无效"))?;
            if bytes.len() > 64 * 1024 {
                return Err(Error::bad("站点图标最大64KiB"));
            }
            self.site_icon = format!(
                "data:image/png;base64,{}",
                STANDARD.encode(
                    crate::avatar::sanitize(&bytes).map_err(|_| Error::bad("图标图片无效"))?
                )
            );
        }
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
        self.check_image_budget()
    }
    fn check_image_budget(&self) -> Api<()> {
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
// Commit only explicitly submitted, already normalized fields against the latest settings.
fn apply_patch(
    latest: &Home,
    normalized: Home,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Api<Home> {
    let mut merged = serde_json::to_value(latest).map_err(anyhow::Error::from)?;
    let normalized = serde_json::to_value(normalized).map_err(anyhow::Error::from)?;
    for key in fields.keys() {
        merged
            .as_object_mut()
            .unwrap()
            .insert(key.clone(), normalized[key].clone());
    }
    let home: Home = serde_json::from_value(merged).map_err(anyhow::Error::from)?;
    home.check_image_budget()?;
    Ok(home)
}
// Only passive markup: no scripts, CSS, media, forms or automatically loaded third-party resources.
fn clean_footer(html: &str) -> String {
    use std::collections::{HashMap, HashSet};
    ammonia::Builder::new()
        .tags(
            [
                "div", "p", "span", "a", "br", "strong", "em", "b", "i", "small", "ul", "ol", "li",
                "code",
            ]
            .into_iter()
            .collect::<HashSet<_>>(),
        )
        .tag_attributes(HashMap::from([("a", HashSet::from(["href", "title"]))]))
        .generic_attributes(HashSet::new())
        .url_schemes(HashSet::from(["https"]))
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer nofollow"))
        .set_tag_attribute_value("a", "target", "_blank")
        .clean(html)
        .to_string()
}
pub async fn public(State(app): State<Arc<App>>) -> Json<Home> {
    let mut home = app.runtime.read().unwrap().settings.home.clone();
    home.footer_html = clean_footer(&home.footer_html);
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
    home: serde_json::Value,
}
pub async fn save(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Update>,
) -> Api<Json<Home>> {
    app.work(move |a| {
        settings::admin(a, &headers)?;
        let password = Zeroizing::new(body.password);
        let session = a.reauthenticate(&headers, &password)?;
        let mut merged = serde_json::to_value(a.runtime.read().unwrap().settings.home.clone())
            .map_err(anyhow::Error::from)?;
        let fields = body
            .home
            .as_object()
            .ok_or_else(|| Error::bad("首页配置无效"))?;
        merged.as_object_mut().unwrap().extend(fields.clone());
        let mut home: Home =
            serde_json::from_value(merged).map_err(|_| Error::bad("首页配置含未知或无效字段"))?;
        home.normalize()?;
        let mut current = a.runtime.write().unwrap();
        let mut updated = current.settings.clone();
        updated.home = apply_patch(&current.settings.home, home, fields)?;
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
    fn partial_commit_preserves_intervening_settings_and_checks_combined_image_budget() {
        let mut stale = Home::default();
        stale.title = "Legacy title".into();
        assert!(stale.normalize().is_ok());
        let mut latest = Home::default();
        latest.site_name = "New branding".into();
        latest.privacy_policy = "New policy".into();
        let fields = serde_json::json!({"title":"Legacy title"});
        let committed = apply_patch(&latest, stale.clone(), fields.as_object().unwrap())
            .ok()
            .unwrap();
        assert_eq!(committed.site_name, "New branding");
        assert_eq!(committed.privacy_policy, "New policy");
        assert_eq!(committed.title, "Legacy title");
        latest.image = "x".repeat(700000);
        stale.announcements = vec![Announcement {
            id: "one".into(),
            title: "One".into(),
            text: String::new(),
            image: "x".into(),
            enabled: true,
        }];
        let fields = serde_json::json!({"announcements":[]});
        assert!(apply_patch(&latest, stale, fields.as_object().unwrap()).is_err());
    }
    #[test]
    fn footer_preserves_passive_links_but_removes_scripts_tracking_and_dangerous_attributes() {
        let output = clean_footer(
            r#"<p id="root" style="position:fixed" onclick="steal()"><strong>朋友</strong><a href="https://beian.miit.gov.cn/" target="_self">备案号</a><a href="javascript:alert(1)">bad</a><a href="&#x6a;avascript:alert(1)">obfuscated</a><a href="data:text/html,bad">data</a><a href="//evil.example">relative</a><img src="https://evil.example/track"><iframe src="https://evil.example"></iframe><svg onload="steal()"></svg><form action="https://evil.example"><input name="password"></form><script>steal()</script><style>body{display:none}</style></p>"#,
        );
        assert!(output.contains("https://beian.miit.gov.cn/"));
        assert!(output.contains("<strong>朋友</strong>"));
        assert!(output.contains("target=\"_blank\""));
        assert!(output.contains("noopener noreferrer nofollow"));
        for forbidden in [
            "javascript:",
            "data:",
            "evil.example",
            "onclick",
            "onload",
            "<img",
            "<iframe",
            "<form",
            "<input",
            "<svg",
            "<script",
            "<style",
            "style=",
            "id=",
            "steal()",
        ] {
            assert!(!output.contains(forbidden), "{forbidden}");
        }
    }
    #[test]
    fn old_home_settings_receive_policy_defaults_and_active_icons_are_rejected() {
        let mut home: Home = serde_json::from_value(
            serde_json::json!({"title":"Old home","subtitle":"","image":"","announcements":[]}),
        )
        .unwrap();
        assert_eq!(home.site_name, "WebTS");
        assert!(home.privacy_policy.contains("私钥"));
        assert!(home.terms.contains("重大过失"));
        assert!(home.normalize().is_ok());
        home.site_icon = "data:image/svg+xml;base64,PHN2Zy8+".into();
        assert!(home.normalize().is_err());
    }
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
