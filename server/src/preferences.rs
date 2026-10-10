//! Account-scoped non-secret preferences. Hardware identifiers and credentials are rejected.
use crate::{
    app::{Api, App, Error},
    db::now,
};
use axum::{Json, extract::State, http::HeaderMap};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
fn session_valid(db: &rusqlite::Connection, s: &crate::db::Session) -> Api<()> {
    let active:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.hash=? AND s.user_id=? AND s.expires>? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?)",params![s.hash,s.user.id,now(),now()],|r|r.get(0)).map_err(anyhow::Error::from)?;
    if !active {
        return Err(Error(axum::http::StatusCode::UNAUTHORIZED, "登录已失效"));
    }
    Ok(())
}
fn valid(key: &str, v: &Value) -> bool {
    let range = |min: f64, max: f64| {
        v.as_f64()
            .is_some_and(|n| n.is_finite() && n >= min && n <= max)
    };
    match key {
        "theme" => matches!(v.as_str(), Some("light" | "dark")),
        "mode" => matches!(v.as_str(), Some("open" | "ptt")),
        "friendsDisplay" => matches!(v.as_str(), Some("floating" | "docked")),
        "pttKey" => v.as_str().is_some_and(|s| {
            s.len() <= 12
                && (s
                    .strip_prefix("Key")
                    .is_some_and(|s| s.len() == 1 && s.bytes().all(|b| b.is_ascii_uppercase()))
                    || s.strip_prefix("Digit")
                        .or_else(|| s.strip_prefix("Numpad"))
                        .is_some_and(|s| s.len() == 1 && s.bytes().all(|b| b.is_ascii_digit()))
                    || matches!(
                        s,
                        "Space"
                            | "ShiftLeft"
                            | "ShiftRight"
                            | "ControlLeft"
                            | "ControlRight"
                            | "AltLeft"
                            | "AltRight"
                    )
                    || s.strip_prefix('F')
                        .and_then(|s| s.parse::<u8>().ok())
                        .is_some_and(|n| (1..=12).contains(&n) && s == format!("F{n}")))
        }),
        "burn" => v.is_boolean(),
        "burnSeconds" => v.as_i64().is_some_and(|n| {
            [60, 600, 1800, 3600, 43200, 86400, 172800, 345600, 604800].contains(&n)
        }),
        "previewHeight" => range(80., 420.),
        "sounds" => v.as_object().is_some_and(|o| {
            o.len() == 2
                && o.get("enabled").is_some_and(Value::is_boolean)
                && o.get("volume")
                    .and_then(Value::as_f64)
                    .is_some_and(|n| (0.0..=1.0).contains(&n))
        }),
        "audio" => v.as_object().is_some_and(|o| {
            o.len() == 11
                && o.iter().all(|(k, v)| match k.as_str() {
                    "noise" => matches!(v.as_str(), Some("off" | "rnnoise")),
                    "keyboard" | "voiceOnly" | "echo" | "autoGain" | "receiveAutoGain"
                    | "typing" => v.is_boolean(),
                    "gain" => v.as_f64().is_some_and(|n| (0.0..=2.0).contains(&n)),
                    "volume" | "strength" | "ducking" => {
                        v.as_f64().is_some_and(|n| (0.0..=1.0).contains(&n))
                    }
                    _ => false,
                })
        }),
        "layout" => v.as_object().is_some_and(|o| {
            o.len() == 2
                && o.get("order").and_then(Value::as_array).is_some_and(|a| {
                    a.len() == 3
                        && ["channels", "chat", "members"]
                            .iter()
                            .all(|id| a.iter().filter(|v| v.as_str() == Some(id)).count() == 1)
                })
                && o.get("widths").and_then(Value::as_object).is_some_and(|w| {
                    w.len() == 3
                        && ["channels", "chat", "members"].iter().all(|id| {
                            w.get(*id).and_then(Value::as_f64).is_some_and(|n| {
                                ((if *id == "chat" { 30.0 } else { 15.0 })..=70.0).contains(&n)
                            })
                        })
                        && (w.values().filter_map(Value::as_f64).sum::<f64>() - 100.).abs() < 0.01
                })
        }),
        _ => false,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    account: i64,
    patch: Value,
    #[serde(default)]
    initialize: bool,
}
pub async fn get(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move|a|{let s=a.session(&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;let value:Option<String>=db.query_row("SELECT data FROM account_preferences WHERE user_id=?",[s.user.id],|r|r.get(0)).optional().map_err(anyhow::Error::from)?;Ok(Json(json!({"account":s.user.id,"initialized":value.is_some(),"preferences":value.and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(json!({}))}))) }).await
}
pub async fn update(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Update>,
) -> Api<Json<Value>> {
    let Some(patch) = body.patch.as_object() else {
        return Err(Error::bad("偏好格式无效"));
    };
    if patch.len() > 10
        || body.patch.to_string().len() > 4096
        || patch.iter().any(|(k, v)| !valid(k, v))
    {
        return Err(Error::bad("偏好包含无效值或不允许同步的数据"));
    }
    app.work(move|a|{let s=a.session(&headers)?;if s.user.id!=body.account{return Err(Error::bad("账号已变化，未保存旧页面设置"));}let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(anyhow::Error::from)?;
        session_valid(&tx,&s)?;
        let old:Option<String>=tx.query_row("SELECT data FROM account_preferences WHERE user_id=?",[s.user.id],|r|r.get(0)).optional().map_err(anyhow::Error::from)?;let mut data=old.as_ref().and_then(|s|serde_json::from_str::<Value>(s).ok()).unwrap_or(json!({}));
        if !(body.initialize&&old.is_some()){for (k,v) in body.patch.as_object().unwrap(){data[k]=v.clone();}tx.execute("INSERT INTO account_preferences(user_id,data) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET data=excluded.data",params![s.user.id,data.to_string()]).map_err(anyhow::Error::from)?;}
        tx.commit().map_err(anyhow::Error::from)?;Ok(Json(json!({"account":s.user.id,"initialized":true,"preferences":data})))
    }).await
}
