use crate::{
    app::{Api, App, Error},
    db::{Session, now},
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub display_name: String,
    pub about: String,
    pub avatar: String,
    pub sync_avatar: bool,
    pub sync_about: bool,
}
impl Profile {
    fn normalize(&mut self) -> Api<()> {
        self.display_name = self.display_name.trim().to_owned();
        if (!self.display_name.is_empty() && !(3..=30).contains(&self.display_name.chars().count()))
            || self.display_name.chars().any(char::is_control)
            || self.about.len() > 512
            || self.about.chars().any(|c| c.is_control() && c != '\n')
        {
            return Err(Error::bad(
                "昵称需3–30个字符；介绍最多512字节，不允许控制字符",
            ));
        }
        if !self.avatar.is_empty() {
            if self.avatar.len() > 87384 {
                return Err(Error::bad("头像最大64KiB"));
            }
            let bytes = STANDARD
                .decode(&self.avatar)
                .map_err(|_| Error::bad("头像编码无效"))?;
            if bytes.len() > crate::avatar::MAX_UPLOAD {
                return Err(Error::bad("头像最大64KiB"));
            }
            self.avatar = STANDARD
                .encode(crate::avatar::sanitize(&bytes).map_err(|_| Error::bad("头像图片无效"))?);
        }
        Ok(())
    }
}
fn read(app: &App, session: &Session) -> Api<Profile> {
    let text: Option<String> = app
        .db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT data FROM profiles WHERE user_id=?",
            [session.user.id],
            |r| r.get(0),
        )
        .optional()
        .map_err(anyhow::Error::from)?;
    Ok(text
        .map(|v| serde_json::from_str(&v))
        .transpose()
        .map_err(anyhow::Error::from)?
        .unwrap_or_default())
}
pub async fn get(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Profile>> {
    let session = app.session(&headers)?;
    Ok(Json(read(&app, &session)?))
}
pub async fn save(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Profile>,
) -> Api<Json<Profile>> {
    app.work(move |a| {
        let session = a.session(&headers)?;
        body.normalize()?;
        let data = serde_json::to_string(&body).map_err(anyhow::Error::from)?;
        let changed = a.db.connection.lock().unwrap().execute(
            "INSERT INTO profiles(user_id,data) SELECT ?,? WHERE EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.hash=? AND u.id=? AND s.expires>? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?) ON CONFLICT(user_id) DO UPDATE SET data=excluded.data",
            params![session.user.id, data, session.hash, session.user.id, now(), now()]
        ).map_err(anyhow::Error::from)?;
        if changed != 1 { return Err(Error(StatusCode::UNAUTHORIZED, "登录已失效，请重新登录")); }
        Ok(Json(body))
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_active_images_and_protocol_controls() {
        let mut profile = Profile {
            display_name: "safe name".into(),
            avatar: STANDARD.encode(b"<svg onload='alert(1)'/>"),
            ..Default::default()
        };
        assert!(profile.normalize().is_err());
        profile.avatar.clear();
        profile.about = "inject\0command".into();
        assert!(profile.normalize().is_err());
        profile.about = "好".repeat(171);
        assert!(profile.normalize().is_err());
        profile.about = "个人介绍\n第二行".into();
        assert!(profile.normalize().is_ok());
    }
}
