//! Bounded, aggregate-only operational visibility; no user data or credentials.
use crate::{
    app::{Api, App, Error},
    db::now,
    settings::admin,
};
use axum::{Json, extract::State, http::HeaderMap};
use serde_json::{Value, json};
use std::{io::Read, sync::Arc};

fn bundle(html: &str) -> Option<&str> {
    html.split(['\"', '\'']).find(|part| {
        part.strip_prefix("/assets/index-")
            .and_then(|v| v.strip_suffix(".js"))
            .is_some_and(|v| {
                !v.is_empty()
                    && v.len() <= 100
                    && v.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
    })
}
pub async fn client_build(State(app): State<Arc<App>>) -> Api<Json<Value>> {
    app.work(|a| {
        let file = std::fs::File::open(std::path::Path::new(&a.config.web_dir).join("index.html"))
            .map_err(anyhow::Error::from)?;
        let mut html = String::new();
        file.take(512 * 1024 + 1)
            .read_to_string(&mut html)
            .map_err(anyhow::Error::from)?;
        if html.len() > 512 * 1024 {
            return Err(Error::bad("界面文件超出限制"));
        }
        Ok(Json(
            json!({"bundle":bundle(&html),"version":env!("CARGO_PKG_VERSION"),"season":a.runtime.read().unwrap().settings.home.season}),
        ))
    })
    .await
}
pub async fn health(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a|{
        let s=admin(a,&headers)?;
        let mut result=a.operational_snapshot();
        let db=a.db.connection.lock().unwrap();
        crate::preferences::session_valid(&db,&s)?;
        let current_admin:bool=db.query_row("SELECT is_admin FROM users WHERE id=?",[s.user.id],|r|r.get(0)).map_err(anyhow::Error::from)?;
        if !current_admin{return Err(Error(axum::http::StatusCode::FORBIDDEN,"管理员权限已撤销"));}
        let (pending,failed):(i64,i64)=db.query_row("SELECT COALESCE(SUM(mail_state IN('pending','sending')),0),COALESCE(SUM(mail_state='failed'),0) FROM notice_receipts",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(anyhow::Error::from)?;
        let (expired,staged):(i64,i64)=db.query_row("SELECT COALESCE(SUM(expires<=?),0),COALESCE(SUM(ready=0),0) FROM friend_messages",[now()],|r|Ok((r.get(0)?,r.get(1)?))).map_err(anyhow::Error::from)?;
        let retries:i64=db.query_row("SELECT COUNT(*) FROM storage_cleanup_retries",[],|r|r.get(0)).map_err(anyhow::Error::from)?;
        result["mail"]=json!({"notice_pending":pending,"notice_failed":failed});
        result["storage"]=json!({"expired_metadata":expired,"staged_metadata":staged,"cleanup_retries":retries});
        result["server_time"]=json!(now());
        Ok(Json(result))
    }).await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_public_asset_paths_are_reported() {
        assert_eq!(
            bundle("<script src=\"/assets/index-ab_12.js\"></script>"),
            Some("/assets/index-ab_12.js")
        );
        for html in [
            "/private/key",
            "\"/assets/index-../../key.js\"",
            "\"https://evil/assets/index-a.js\"",
            "\"/assets/index-a.js?secret=one\"",
        ] {
            assert_eq!(bundle(html), None);
        }
    }
}
