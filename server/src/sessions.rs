//! Account-owned login sessions. Public references are never login credentials.
use crate::{
    app::{Api, App, Error},
    db::now,
    preferences::session_valid,
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::Zeroizing;

pub(crate) fn migrate(db: &mut Connection) -> anyhow::Result<()> {
    let tx = db.transaction()?;
    tx.execute_batch("CREATE INDEX IF NOT EXISTS sessions_owner_expiry ON sessions(user_id,expires);
      CREATE TABLE IF NOT EXISTS session_details(hash TEXT PRIMARY KEY REFERENCES sessions(hash) ON DELETE CASCADE,id TEXT UNIQUE NOT NULL,created_at INTEGER NOT NULL,browser TEXT NOT NULL,platform TEXT NOT NULL,remembered INTEGER CHECK(remembered IN(0,1)));
      INSERT INTO session_details(hash,id,created_at,browser,platform,remembered) SELECT hash,lower(hex(randomblob(32))),0,'','',NULL FROM sessions WHERE hash NOT IN(SELECT hash FROM session_details);")?;
    if !tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('session_details') WHERE name='public_device')",[],|r|r.get::<_,bool>(0))? {
        tx.execute_batch("ALTER TABLE session_details ADD COLUMN public_device INTEGER NOT NULL DEFAULT 0 CHECK(public_device IN(0,1))")?;
    }
    tx.execute_batch("CREATE TABLE IF NOT EXISTS login_names(user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,name TEXT UNIQUE NOT NULL);
      CREATE TABLE IF NOT EXISTS quick_logins(id TEXT PRIMARY KEY,poll_hash TEXT NOT NULL,approve_hash TEXT NOT NULL,code TEXT NOT NULL,expires INTEGER NOT NULL,policy TEXT NOT NULL,browser TEXT NOT NULL,platform TEXT NOT NULL,authorizer TEXT REFERENCES sessions(hash) ON DELETE CASCADE);
      CREATE INDEX IF NOT EXISTS quick_login_expiry ON quick_logins(expires);")?;
    tx.commit()?;
    Ok(())
}

// Only these coarse names are persisted; no IP, raw User-Agent or fingerprint.
pub(crate) fn client_label(headers: &HeaderMap) -> (&'static str, &'static str) {
    let ua = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .filter(|ua| ua.len() <= 1024)
        .unwrap_or("");
    let browser = if ua.contains("Edg/") || ua.contains("EdgiOS/") || ua.contains("EdgA/") {
        "Edge"
    } else if ua.contains("Firefox/") || ua.contains("FxiOS/") {
        "Firefox"
    } else if ua.contains("Chrome/") || ua.contains("CriOS/") {
        "Chrome"
    } else if ua.contains("Safari/") {
        "Safari"
    } else {
        "未知浏览器"
    };
    let platform = if ua.contains("Android") {
        "Android"
    } else if ua.contains("iPhone") || ua.contains("iPad") {
        "iOS / iPadOS"
    } else if ua.contains("Windows") {
        "Windows"
    } else if ua.contains("Macintosh") {
        "macOS / iPadOS"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        "未知系统"
    };
    (browser, platform)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    account: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    account: i64,
    target: String,
    password: String,
}
fn scope(actual: i64, expected: i64) -> Api<()> {
    if actual != expected {
        return Err(Error(
            StatusCode::CONFLICT,
            "账号已变化，请重新打开安全中心",
        ));
    }
    Ok(())
}

pub async fn list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<Listing>,
) -> Api<Json<Value>> {
    app.work(move|a|{
        let s=a.session(&headers)?;scope(s.user.id,q.account)?;
        let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let total:i64=db.query_row("SELECT count(*) FROM sessions WHERE user_id=? AND expires>?",params![s.user.id,now()],|r|r.get(0)).map_err(anyhow::Error::from)?;
        let mut stmt=db.prepare("SELECT d.id,d.created_at,d.browser,d.platform,d.remembered,s.expires,s.hash=?,d.public_device FROM sessions s JOIN session_details d ON d.hash=s.hash WHERE s.user_id=? AND s.expires>? ORDER BY (s.hash=?) DESC,d.created_at DESC,d.id LIMIT 50").map_err(anyhow::Error::from)?;
        let items=stmt.query_map(params![s.hash,s.user.id,now(),s.hash],|r|Ok(json!({"id":r.get::<_,String>(0)?,"created_at":r.get::<_,i64>(1)?,"browser":r.get::<_,String>(2)?,"platform":r.get::<_,String>(3)?,"remembered":r.get::<_,Option<bool>>(4)?,"expires":r.get::<_,i64>(5)?,"current":r.get::<_,bool>(6)?,"public_device":r.get::<_,bool>(7)?}))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
        Ok(Json(json!({"account":s.user.id,"items":items,"total":total,"server_time":now()})))
    }).await
}

pub async fn revoke(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Revocation>,
) -> Api<Json<Value>> {
    if body.password.len() > 128
        || !(body.target == "others"
            || body.target.len() == 64 && body.target.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(Error::bad("会话编号或密码格式无效"));
    }
    app.expensive(move|a|{
        // Check account before password work, then authorize again under the write transaction.
        scope(a.session(&headers)?.user.id,body.account)?;
        let s=a.reauthenticate(&headers,&Zeroizing::new(body.password))?;
        scope(s.user.id,body.account)?;
        let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(anyhow::Error::from)?;session_valid(&tx,&s)?;
        let hash=if body.target=="others" {None} else {
            let hash:Option<String>=tx.query_row("SELECT s.hash FROM sessions s JOIN session_details d ON d.hash=s.hash WHERE d.id=? AND s.user_id=? AND s.expires>?",params![body.target,s.user.id,now()],|r|r.get(0)).optional().map_err(anyhow::Error::from)?;
            let hash=hash.ok_or(Error(StatusCode::NOT_FOUND,"会话已退出或不存在"))?;
            if hash==s.hash{return Err(Error::bad("请使用退出当前设备结束本次登录"));}Some(hash)
        };
        let removed=if let Some(hash)=&hash {tx.execute("DELETE FROM sessions WHERE user_id=? AND hash=?",params![s.user.id,hash])}
            else{tx.execute("DELETE FROM sessions WHERE user_id=? AND hash<>?",params![s.user.id,s.hash])}.map_err(anyhow::Error::from)?;
        tx.commit().map_err(anyhow::Error::from)?;
        // Keep the DB guard until cancellation: a newly issued session must not be cancelled by this operation.
        if let Some(hash)=hash {a.connections.cancel(s.user.id,Some(&hash),None);}else{a.connections.cancel_other_sessions(s.user.id,&s.hash);}
        Ok(Json(json!({"account":s.user.id,"removed":removed})))
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_sessions_migrate_idempotently_without_changing_credentials() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE sessions(hash TEXT PRIMARY KEY,user_id INTEGER NOT NULL,expires INTEGER NOT NULL);INSERT INTO sessions VALUES('legacy-hash',7,123);").unwrap();
        migrate(&mut db).unwrap();
        migrate(&mut db).unwrap();
        let(id,created,browser,remembered):(String,i64,String,Option<bool>)=db.query_row("SELECT id,created_at,browser,remembered FROM session_details WHERE hash='legacy-hash'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        assert_eq!(id.len(), 64);
        assert_eq!(created, 0);
        assert!(browser.is_empty());
        assert_eq!(remembered, None);
        // A previous binary can still use its original three-column INSERT.
        db.execute("INSERT INTO sessions VALUES('old-binary-login',7,456)", [])
            .unwrap();
        migrate(&mut db).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT id FROM session_details WHERE hash='legacy-hash'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            id
        );
        db.execute("DELETE FROM sessions WHERE hash='legacy-hash'", [])
            .unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM session_details", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn browser_labels_never_persist_raw_or_unbounded_input() {
        let mut headers = HeaderMap::new();
        for (ua, browser, platform) in [
            ("Mozilla Windows Chrome/1 Safari/1 Edg/1", "Edge", "Windows"),
            ("iPhone CriOS/1 Safari/1", "Chrome", "iOS / iPadOS"),
            ("Android Firefox/1", "Firefox", "Android"),
            ("<script>steal()</script>", "未知浏览器", "未知系统"),
        ] {
            headers.insert("user-agent", ua.parse().unwrap());
            assert_eq!(client_label(&headers), (browser, platform));
        }
        headers.insert(
            "user-agent",
            format!("Chrome/{}", "x".repeat(1024)).parse().unwrap(),
        );
        assert_eq!(client_label(&headers), ("未知浏览器", "未知系统"));
    }
}
