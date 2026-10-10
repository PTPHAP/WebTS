//! Account friendships and expiring encrypted envelopes; ordinary messages use encrypted object storage without peer consent.
use crate::{
    app::{Api, App, Error},
    db::{Session, now},
    storage::{self, MAX_CIPHERTEXT},
    vault::token,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use zeroize::Zeroizing;

fn db_error(e: rusqlite::Error) -> Error {
    anyhow::Error::from(e).into()
}
fn current(a: &App, headers: &HeaderMap) -> Api<Session> {
    a.session(headers)
}
fn session_valid(db: &rusqlite::Connection, s: &Session) -> Api<()> {
    let valid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sessions ss JOIN users u ON u.id=ss.user_id WHERE ss.hash=? AND u.id=? AND ss.expires>? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?)",params![s.hash,s.user.id,now(),now()],|r|r.get(0)).map_err(db_error)?;
    if !valid {
        return Err(Error(StatusCode::UNAUTHORIZED, "登录已失效"));
    }
    Ok(())
}
fn pair(a: i64, b: i64) -> (i64, i64) {
    (a.min(b), a.max(b))
}
fn b64(value: &str, bytes: usize) -> bool {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(value)
        .is_ok_and(|v| v.len() == bytes && v.iter().any(|&b| b != 0))
}
fn valid_identity(value: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(value) else {
        return false;
    };
    value.len() < 256
        && v["version"] == 1
        && v["curve"].as_str().is_some_and(|s| b64(s, 32))
        && v["ed"].as_str().is_some_and(|s| b64(s, 32))
        && v.as_object().is_some_and(|v| v.len() == 3)
}
fn valid_ciphertext(value: &str) -> bool {
    if value.len() > MAX_CIPHERTEXT {
        return false;
    }
    let Ok(v) = serde_json::from_str::<Value>(value) else {
        return false;
    };
    v["version"] == 1
        && v["packet"].is_string()
        && v["device"].as_str().is_some_and(uuid)
        && v["content"]
            .as_str()
            .is_some_and(|s| s.len() < 256 * 1024 && b64_any(s))
        && v["iv"].as_str().is_some_and(|s| b64(s, 12))
}
fn b64_any(value: &str) -> bool {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .is_ok()
}
fn device(db: &rusqlite::Connection, s: &Session, headers: &HeaderMap) -> Api<()> {
    let value = headers
        .get("x-webts-chat-device")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if value.len() != 64 {
        return Err(Error(StatusCode::CONFLICT, "请先解锁此设备的加密私信"));
    }
    let ok:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM friend_keys k JOIN sessions ss ON ss.user_id=k.user_id JOIN users u ON u.id=k.user_id WHERE k.user_id=? AND k.lease_hash=? AND ss.hash=? AND ss.expires>? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?)",params![s.user.id,crate::db::digest(value),s.hash,now(),now()],|r|r.get(0)).map_err(db_error)?;
    if !ok {
        return Err(Error(
            StatusCode::CONFLICT,
            "私信已由另一设备接管或登录已失效，请重新解锁",
        ));
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    password: String,
    lease: String,
    device: String,
    payload: String,
    signature: String,
}
pub async fn activate(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Device>,
) -> Api<Json<Value>> {
    app.work(move|a| {
        let automatic=body.password.is_empty();
        let s=if automatic {let s=current(a,&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;device(&db,&s,&headers)?;let old:String=db.query_row("SELECT device FROM friend_keys WHERE user_id=?",[s.user.id],|r|r.get(0)).map_err(db_error)?;if old!=body.device{return Err(Error::bad("设备接管需要重新验证密码"));}s} else {a.reauthenticate(&headers,&Zeroizing::new(body.password))?};
        if body.lease.len()!=64||!body.lease.bytes().all(|b|b.is_ascii_hexdigit())||!uuid(&body.device)||body.payload.len()>8192||!b64(&body.signature,64){return Err(Error::bad("设备密钥数据无效"));}
        let payload:Value=serde_json::from_str(&body.payload).map_err(|_|Error::bad("设备密钥格式无效"))?;
        let identity=payload["identity"].as_str().unwrap_or("");
        let keys=payload["keys"].as_array().filter(|v|!v.is_empty()&&v.len()<=64).ok_or(Error::bad("需要1–64个一次性公钥"))?;
        if keys.iter().any(|k|!k.as_str().is_some_and(|v|b64(v,32))){return Err(Error::bad("一次性公钥无效"));}
        let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(db_error)?;session_valid(&tx,&s)?;
        let registered:String=tx.query_row("SELECT public_key FROM friend_keys WHERE user_id=?",[s.user.id],|r|r.get(0)).map_err(db_error)?;
        if registered!=identity{return Err(Error(StatusCode::CONFLICT,"设备与已登记身份不一致"));}
        let root:Value=serde_json::from_str(identity).map_err(anyhow::Error::from)?;
        if !webts_crypto::Engine::verify(root["ed"].as_str().unwrap_or(""),&body.payload,&body.signature){return Err(Error::bad("一次性公钥的身份签名无效"));}
        // The peer independently verifies this signature against their pinned identity.
        let bundle=json!({"payload":body.payload,"signature":body.signature,"device":body.device}).to_string();
        let old_device:String=tx.query_row("SELECT device FROM friend_keys WHERE user_id=?",[s.user.id],|r|r.get(0)).map_err(db_error)?;
        // Fence an automatic renewal again inside the write transaction: another
        // device may have taken over while this request's signature was checked.
        if automatic {device(&tx,&s,&headers)?;if old_device!=body.device{return Err(Error::bad("设备接管需要重新验证密码"));}}
        if !old_device.is_empty()&&old_device!=body.device {
            // A backup-based device lacks old ratchets. Do not deliver late old-device
            // packets into a new session for the same public identity.
            tx.execute("UPDATE friend_messages SET expires=min(expires,?) WHERE mode='e2ee' AND (sender=? OR recipient=?)",params![now(),s.user.id,s.user.id]).map_err(db_error)?;
        }
        tx.execute("UPDATE friend_keys SET device=?,lease_hash=? WHERE user_id=?",params![body.device,crate::db::digest(&body.lease),s.user.id]).map_err(db_error)?;
        tx.execute("DELETE FROM friend_prekeys WHERE user_id=? AND used=0",[s.user.id]).map_err(db_error)?;
        let count:i64=tx.query_row("SELECT count(*) FROM friend_prekeys WHERE user_id=?",[s.user.id],|r|r.get(0)).map_err(db_error)?;
        if count+keys.len() as i64>10000{return Err(Error(StatusCode::TOO_MANY_REQUESTS,"设备公钥容量达到上限"));}
        for key in keys {tx.execute("INSERT OR IGNORE INTO friend_prekeys(user_id,key,device,bundle,used) VALUES(?,?,?,?,0)",params![s.user.id,key.as_str(),body.device,bundle]).map_err(db_error)?;}
        tx.commit().map_err(db_error)?;
        Ok(Json(json!({"message":"加密私信由此设备处理；其他设备需要重新解锁接管。"})))
    }).await
}
pub async fn claim(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(peer): Path<i64>,
) -> Api<Json<Value>> {
    app.work(move|a| {
        let s=current(a,&headers)?;let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(db_error)?;device(&tx,&s,&headers)?;accepted(&tx,s.user.id,peer)?;
        let row:Option<(String,String)>=tx.query_row("SELECT p.key,p.bundle FROM friend_prekeys p JOIN friend_keys k ON k.user_id=p.user_id AND k.device=p.device WHERE p.user_id=? AND p.used=0 ORDER BY p.rowid LIMIT 1",[peer],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
        let (key,bundle)=row.ok_or(Error(StatusCode::CONFLICT,"对方需要先解锁并更新设备一次性密钥"))?;
        tx.execute("UPDATE friend_prekeys SET used=1 WHERE user_id=? AND key=? AND used=0",params![peer,key]).map_err(db_error)?;tx.commit().map_err(db_error)?;
        Ok(Json(json!({"key":key,"bundle":serde_json::from_str::<Value>(&bundle).map_err(anyhow::Error::from)?})))
    }).await
}
fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
            }
        })
}
fn accepted(db: &rusqlite::Connection, owner: i64, peer: i64) -> Api<()> {
    let (lo, hi) = pair(owner, peer);
    let ok:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM friendships f JOIN users u ON u.id=? WHERE f.lo=? AND f.hi=? AND f.status=1 AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?)",params![peer,lo,hi,now()],|r|r.get(0)).map_err(db_error)?;
    if !ok {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "仅能向已接受且未封禁的好友发送或读取私信",
        ));
    }
    Ok(())
}
fn encryption_state(db: &rusqlite::Connection, owner: i64, peer: i64) -> Api<(bool, bool, i64)> {
    let (lo, hi) = pair(owner, peer);
    let (a, b, epoch) = db
        .query_row(
            "SELECT allow_lo,allow_hi,epoch FROM friend_modes WHERE lo=? AND hi=?",
            params![lo, hi],
            |r| {
                Ok((
                    r.get::<_, bool>(0)?,
                    r.get::<_, bool>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(db_error)?
        .unwrap_or((true, true, 0));
    Ok(if owner == lo {
        (a, b, epoch)
    } else {
        (b, a, epoch)
    })
}
fn require_mode(
    db: &rusqlite::Connection,
    owner: i64,
    peer: i64,
    mode: &str,
    epoch: i64,
) -> Api<()> {
    let (own, _, current) = encryption_state(db, owner, peer)?;
    if epoch != current || mode != if own { "server" } else { "e2ee" } {
        return Err(Error(
            StatusCode::CONFLICT,
            "发送方式已变化，请刷新后重试；不会自动转换待发消息",
        ));
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Encryption {
    server_decrypt: bool,
    #[serde(default)]
    #[serde(rename = "acknowledge")]
    _acknowledge: bool,
    #[serde(default)]
    password: String,
}
pub async fn encryption(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(peer): Path<i64>,
    Json(body): Json<Encryption>,
) -> Api<Json<Value>> {
    if body.password.len() > 128 {
        return Err(Error::bad("密码输入超出限制"));
    }
    app.work(move |a| {
        let s = if !body.password.is_empty() { a.reauthenticate(&headers,&Zeroizing::new(body.password))? } else { current(a,&headers)? };
        let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(db_error)?;session_valid(&tx,&s)?;accepted(&tx,s.user.id,peer)?;
        let (lo,hi)=pair(s.user.id,peer);let (own,_,epoch)=encryption_state(&tx,s.user.id,peer)?;
        if own!=body.server_decrypt {
            tx.execute("INSERT OR IGNORE INTO friend_modes(lo,hi,allow_lo,allow_hi) VALUES(?,?,1,1)",params![lo,hi]).map_err(db_error)?;
            let sql=if s.user.id==lo {"UPDATE friend_modes SET allow_lo=?,epoch=epoch+1 WHERE lo=? AND hi=?"} else {"UPDATE friend_modes SET allow_hi=?,epoch=epoch+1 WHERE lo=? AND hi=?"};
            tx.execute(sql,params![body.server_decrypt,lo,hi]).map_err(db_error)?;
            // A pending upload cannot become visible after either person's choice changes.
            tx.execute("UPDATE friend_messages SET expires=min(expires,?) WHERE ready=0 AND ((sender=? AND recipient=?) OR (sender=? AND recipient=?))",params![now(),lo,hi,hi,lo]).map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        Ok(Json(json!({"mode":if body.server_decrypt {"server"}else{"e2ee"},"epoch":epoch+i64::from(own!=body.server_decrypt),"message":"发送方式已保存，无需等待对方确认。旧消息保护方式保持。"})))
    }).await
}
pub async fn me(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a| {
        let s=current(a,&headers)?; let storage=a.runtime.read().unwrap().settings.storage.clone(); let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        db.execute("INSERT OR IGNORE INTO friend_keys(user_id,code,public_key) VALUES(?,?,'')",params![s.user.id,&token()?[..24]]).map_err(db_error)?;
        let (code,key,share):(String,String,bool)=db.query_row("SELECT code,public_key,share_presence FROM friend_keys WHERE user_id=?",[s.user.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db_error)?;a.connections.touch(s.user.id,&s.hash);
        let left:i64=db.query_row("SELECT count(*) FROM friend_prekeys WHERE user_id=? AND used=0",[s.user.id],|r|r.get(0)).map_err(db_error)?;
        Ok(Json(json!({"id":s.user.id,"code":code,"public_key":key,"share_presence":share,"prekeys_left":left,"storage_enabled":storage.enabled,"retention_days":if storage.retention_days==0{7}else{storage.retention_days}})))
    }).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    password: String,
    public_key: String,
}
pub async fn set_key(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Key>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let s=a.reauthenticate(&headers,&Zeroizing::new(body.password))?;
        let key=body.public_key.trim();
        if !valid_identity(key) {return Err(Error::bad("需要有效的Olm公开身份，不能上传私钥"));}
        let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let old:Option<String>=db.query_row("SELECT public_key FROM friend_keys WHERE user_id=?",[s.user.id],|r|r.get(0)).optional().map_err(db_error)?;
        if old.as_ref().is_some_and(|v|!v.is_empty()&&v!=key) {return Err(Error(StatusCode::CONFLICT,"账号已绑定私信公钥，请导入原设备备份；不允许静默替换密钥"));}
        // Check session again inside the same SQLite write statement after password hashing.
        let changed=db.execute("INSERT INTO friend_keys(user_id,code,public_key) SELECT ?,?,? WHERE EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.hash=? AND u.id=? AND s.expires>? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?) ON CONFLICT(user_id) DO UPDATE SET public_key=excluded.public_key",params![s.user.id,&token()?[..24],key,s.hash,s.user.id,now(),now()]).map_err(db_error)?;
        if changed!=1{return Err(Error(StatusCode::UNAUTHORIZED,"登录已失效"));}
        Ok(Json(json!({"message":"公钥已登记，私钥只在设备中保存。"})))
    }).await
}
pub async fn list(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a| {
        let s=current(a,&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let mut statement=db.prepare("SELECT u.id,COALESCE(json_extract(p.data,'$.display_name'),''),k.public_key,f.requester,f.status,f.blocked_by,k.device,CASE WHEN f.lo=? THEN COALESCE(m.allow_lo,1) ELSE COALESCE(m.allow_hi,1) END,CASE WHEN f.lo=? THEN COALESCE(m.allow_hi,1) ELSE COALESCE(m.allow_lo,1) END,COALESCE(m.epoch,0),COALESCE(p.avatar_hash,''),COALESCE(json_extract(p.data,'$.about'),'') FROM friendships f JOIN users u ON u.id=CASE WHEN f.lo=? THEN f.hi ELSE f.lo END JOIN friend_keys k ON k.user_id=u.id LEFT JOIN profiles p ON p.user_id=u.id LEFT JOIN friend_modes m ON m.lo=f.lo AND m.hi=f.hi WHERE (f.lo=? OR f.hi=?) AND (f.status!=2 OR f.blocked_by=?) AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=? ORDER BY f.status,u.id LIMIT 132").map_err(db_error)?;
        let mut rows=statement.query_map(params![s.user.id,s.user.id,s.user.id,s.user.id,s.user.id,s.user.id,now()],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"name":r.get::<_,String>(1)?,"public_key":r.get::<_,String>(2)?,"requester":r.get::<_,i64>(3)?,"status":r.get::<_,i64>(4)?,"blocked_by":r.get::<_,i64>(5)?,"device":r.get::<_,String>(6)?,"allow_server":r.get::<_,bool>(7)?,"peer_allow_server":r.get::<_,bool>(8)?,"mode_epoch":r.get::<_,i64>(9)?,"avatar_hash":r.get::<_,String>(10)?,"about":r.get::<_,String>(11)?}))).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)?;
        for row in &mut rows {if row["status"]==1 {let (online,locations)=friend_activity(a,&db,row["id"].as_i64().unwrap())?;row["online"]=json!(online);row["locations"]=json!(locations);}}
        let unread:i64=db.query_row("SELECT count(*) FROM friend_messages m JOIN friendships f ON f.lo=min(m.sender,m.recipient) AND f.hi=max(m.sender,m.recipient) JOIN users u ON u.id=m.sender WHERE m.recipient=? AND m.ready=1 AND m.read_at IS NULL AND m.expires>? AND f.status=1 AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?",params![s.user.id,now(),now()],|r|r.get(0)).map_err(db_error)?;
        let latest:i64=db.query_row("SELECT friend_revision FROM users WHERE id=?",[s.user.id],|r|r.get(0)).map_err(db_error)?;
        Ok(Json(json!({"friends":rows,"unread":unread,"latest_message":latest})))
    }).await
}
fn friend_activity(a: &App, db: &rusqlite::Connection, peer: i64) -> Api<(bool, Vec<Value>)> {
    let share = db
        .query_row(
            "SELECT share_presence FROM friend_keys WHERE user_id=?",
            [peer],
            |r| r.get::<_, bool>(0),
        )
        .optional()
        .map_err(db_error)?
        .unwrap_or(false);
    if !share {
        return Ok((false, Vec::new()));
    }
    let (sessions, locations) = a.connections.activity(peer);
    let valid = |hash: &str| -> Api<bool> {
        db.query_row("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.hash=? AND s.user_id=? AND s.expires>? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?)",params![hash,peer,now(),now()],|r|r.get(0)).map_err(db_error)
    };
    let mut online = false;
    for session in sessions {
        online |= valid(&session)?;
    }
    let runtime = a.runtime.read().unwrap();
    let mut shared = Vec::new();
    for (session, location) in locations {
        let server = location["server"].as_str().unwrap_or("");
        let permitted = if server.is_empty() {
            runtime.settings.allow_custom
        } else {
            runtime
                .settings
                .servers
                .iter()
                .any(|s| s.id == server && s.address == location["address"])
        };
        if permitted && valid(&session)? {
            online = true;
            shared.push(location);
        }
    }
    shared.sort_by(|a, b| a["connection"].as_str().cmp(&b["connection"].as_str()));
    Ok((online, shared))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presence {
    share: bool,
}
pub async fn presence(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Presence>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let s = current(a, &headers)?;
        let db = a.db.connection.lock().unwrap();
        session_valid(&db, &s)?;
        db.execute(
            "UPDATE friend_keys SET share_presence=? WHERE user_id=?",
            params![body.share, s.user.id],
        )
        .map_err(db_error)?;
        Ok(Json(json!({"share_presence":body.share})))
    })
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Join {
    connection: String,
}
pub async fn join(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(peer): Path<i64>,
    Json(body): Json<Join>,
) -> Api<Json<Value>> {
    if body.connection.len() > 128 {
        return Err(Error::bad("连接引用无效"));
    }
    app.work(move |a| {
        let s = current(a, &headers)?;
        let db = a.db.connection.lock().unwrap();
        session_valid(&db, &s)?;
        accepted(&db, s.user.id, peer)?;
        let (_, locations) = friend_activity(a, &db, peer)?;
        let location = locations
            .into_iter()
            .find(|v| v["connection"] == body.connection)
            .ok_or(Error(
                StatusCode::NOT_FOUND,
                "好友已离线、隐藏位置或连接设置已变化",
            ))?;
        Ok(Json(location))
    })
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    code: String,
}
pub async fn avatar(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(peer): Path<i64>,
) -> Api<Response> {
    app.work(move |a| {
        let s=current(a,&headers)?;
        let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let (lo,hi)=pair(s.user.id,peer);
        let data:Option<(String,String)>=db.query_row("SELECT json_extract(p.data,'$.avatar'),p.avatar_hash FROM profiles p JOIN users u ON u.id=p.user_id JOIN friendships f ON f.lo=? AND f.hi=? WHERE p.user_id=? AND f.status IN(0,1) AND length(p.avatar_hash)=64 AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?",params![lo,hi,peer,now()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
        let (avatar,hash)=data.ok_or(Error(StatusCode::NOT_FOUND,"好友头像不可用"))?;
        let tag=format!("\"{hash}\"");
        let mut response_headers=HeaderMap::new();
        response_headers.insert("cache-control","private, no-store".parse().unwrap());
        response_headers.insert("etag",tag.parse().unwrap());
        response_headers.insert("content-type","image/png".parse().unwrap());
        response_headers.insert("x-content-type-options","nosniff".parse().unwrap());
        if headers.get("if-none-match").and_then(|v|v.to_str().ok())==Some(tag.as_str()) { return Ok((StatusCode::NOT_MODIFIED,response_headers).into_response()); }
        use base64::Engine;
        let bytes=base64::engine::general_purpose::STANDARD.decode(avatar).map_err(|_|Error::bad("好友头像不可用"))?;
        if image::guess_format(&bytes).ok()!=Some(image::ImageFormat::Png) {return Err(Error::bad("好友头像不可用"));}
        Ok((response_headers,bytes).into_response())
    }).await
}
pub async fn request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Request>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let s=current(a,&headers)?; if body.code.len()!=24 || !body.code.bytes().all(|b|b.is_ascii_hexdigit()) {return Err(Error::bad("好友码为24位字符"));}
        let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let peer:Option<i64>=db.query_row("SELECT k.user_id FROM friend_keys k JOIN users u ON u.id=k.user_id WHERE k.code=? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?",params![body.code.to_lowercase(),now()],|r|r.get(0)).optional().map_err(db_error)?;
        let peer=peer.filter(|&id|id!=s.user.id).ok_or(Error::bad("好友码不可用"))?;
        db.execute("INSERT OR IGNORE INTO friend_keys(user_id,code,public_key) VALUES(?,?,'')",params![s.user.id,&token()?[..24]]).map_err(db_error)?;
        for id in [s.user.id,peer] {
            let count:i64=db.query_row("SELECT count(*) FROM friendships WHERE (lo=? OR hi=?) AND (status!=2 OR blocked_by=?)",params![id,id,id],|r|r.get(0)).map_err(db_error)?;
            if count>=132{return Err(Error(StatusCode::TOO_MANY_REQUESTS,"好友及请求数量达到上限"));}
        }
        let (lo,hi)=pair(s.user.id,peer);
        let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM friendships WHERE lo=? AND hi=?)",params![lo,hi],|r|r.get(0)).map_err(db_error)?;
        if exists {return Ok(Json(json!({"message":"好友关系或请求已存在。"})));}
        db.execute("INSERT INTO friend_request_limits(user_id,window,count) VALUES(?,?,0) ON CONFLICT(user_id) DO UPDATE SET window=excluded.window,count=0 WHERE window<=?",params![s.user.id,now(),now()-60]).map_err(db_error)?;
        let changed=db.execute("UPDATE friend_request_limits SET count=count+1 WHERE user_id=? AND count<5",[s.user.id]).map_err(db_error)?;
        if changed!=1{return Err(Error(StatusCode::TOO_MANY_REQUESTS,"每分钟最多发送5个好友请求"));}
        db.execute("INSERT OR IGNORE INTO friendships(lo,hi,requester,status,blocked_by,created_at) VALUES(?,?,?,0,0,?)",params![lo,hi,s.user.id,now()]).map_err(db_error)?;
        Ok(Json(json!({"message":"好友请求已提交；对方接受后可核对安全码。"})))
    }).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    action: String,
}
pub async fn action(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(peer): Path<i64>,
    Json(body): Json<Action>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let s=current(a,&headers)?; let (lo,hi)=pair(s.user.id,peer);
        let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let changed=match body.action.as_str() {
            "accept"=>db.execute("UPDATE friendships SET status=1 WHERE lo=? AND hi=? AND status=0 AND requester!=?",params![lo,hi,s.user.id]),
            "decline"|"remove"=>db.execute("DELETE FROM friendships WHERE lo=? AND hi=? AND status!=2",params![lo,hi]),
            "block"=>db.execute("UPDATE friendships SET status=2,blocked_by=? WHERE lo=? AND hi=? AND (status!=2 OR blocked_by=?)",params![s.user.id,lo,hi,s.user.id]),
            "unblock"=>db.execute("DELETE FROM friendships WHERE lo=? AND hi=? AND status=2 AND blocked_by=?",params![lo,hi,s.user.id]),
            _=>return Err(Error::bad("好友操作无效")),
        }.map_err(db_error)?;
        if changed==0{return Err(Error(StatusCode::NOT_FOUND,"请求不存在或已变化"));}
        if body.action!="accept" {db.execute("UPDATE friend_messages SET expires=min(expires,?) WHERE (sender=? AND recipient=?) OR (sender=? AND recipient=?)",params![now(),lo,hi,hi,lo]).map_err(db_error)?;}
        Ok(Json(json!({"message":"好友关系已更新。"})))
    }).await
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    #[serde(default)]
    before: i64,
}
pub async fn messages(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(peer): Path<i64>,
    Query(page): Query<Page>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let s=current(a,&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;accepted(&db,s.user.id,peer)?;let unlocked=device(&db,&s,&headers).is_ok();
        let mut query=db.prepare("SELECT seq,id,sender,recipient,created_at,expires,read_at,burn,mode,mode_epoch,burn_seconds FROM friend_messages WHERE ((sender=? AND recipient=?) OR (sender=? AND recipient=?)) AND ready=1 AND expires>? AND (?=1 OR mode='server') AND (?=0 OR seq<?) ORDER BY seq DESC LIMIT 30").map_err(db_error)?;
        let rows=query.query_map(params![s.user.id,peer,peer,s.user.id,now(),unlocked,page.before,page.before],|r|Ok(json!({"seq":r.get::<_,i64>(0)?,"id":r.get::<_,String>(1)?,"sender":r.get::<_,i64>(2)?,"recipient":r.get::<_,i64>(3)?,"created_at":r.get::<_,i64>(4)?,"expires":r.get::<_,i64>(5)?,"read_at":r.get::<_,Option<i64>>(6)?,"burn":r.get::<_,bool>(7)?,"mode":r.get::<_,String>(8)?,"epoch":r.get::<_,i64>(9)?,"burn_seconds":r.get::<_,Option<i64>>(10)?}))).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)?;
        Ok(Json(json!({"messages":rows})))
    }).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Send {
    peer: i64,
    id: String,
    ciphertext: String,
    #[serde(default)]
    burn: bool,
    #[serde(default)]
    burn_seconds: Option<i64>,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default)]
    epoch: i64,
}
fn default_mode() -> String {
    "e2ee".into()
}
pub async fn send(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Send>,
) -> Api<Json<Value>> {
    let ciphertext = Zeroizing::new(std::mem::take(&mut body.ciphertext));
    if body.burn_seconds.is_some_and(|s| {
        !body.burn || ![60, 600, 1800, 3600, 43200, 86400, 172800, 345600, 604800].contains(&s)
    }) || !uuid(&body.id)
        || body.epoch < 0
        || ciphertext.len() > MAX_CIPHERTEXT
        || !matches!(body.mode.as_str(), "e2ee" | "server")
        || body.mode == "e2ee" && !valid_ciphertext(&ciphertext)
    {
        return Err(Error::bad("只接受限制以内且模式明确的加密消息"));
    }
    let _permit = app
        .friend_objects
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "临时存储正忙，请稍后重试"))?;
    let check_headers = headers.clone();
    let id = body.id.clone();
    let peer = body.peer;
    let target_device = if body.mode == "e2ee" {
        serde_json::from_str::<Value>(&ciphertext).unwrap()["device"]
            .as_str()
            .unwrap()
            .to_owned()
    } else {
        String::new()
    };
    let check_device = target_device.clone();
    let burn = body.burn;
    let burn_seconds = body.burn_seconds;
    let mode = body.mode.clone();
    let epoch = body.epoch;
    let validation = ciphertext.clone();
    let input = Zeroizing::new(if mode == "e2ee" && epoch == 0 {
        format!("{}:{burn}", ciphertext.as_str())
    } else {
        format!("{}:{burn}:{mode}:{epoch}", ciphertext.as_str())
    });
    let hash = crate::db::digest(&if let Some(seconds) = burn_seconds {
        Zeroizing::new(format!("{}:burn_seconds:{seconds}", input.as_str()))
    } else {
        input
    });
    let (owner,key,duplicate,storage)=app.work(move |a| {
        let s=current(a,&check_headers)?;let runtime=a.runtime.read().unwrap();
        let storage=runtime.settings.storage.clone();let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;if mode=="e2ee" {device(&db,&s,&check_headers)?;} accepted(&db,s.user.id,peer)?;
        require_mode(&db,s.user.id,peer,&mode,epoch)?;
        if mode=="server" {crate::friend_content::validate(&validation,&a.config.origin(),&id,s.user.id,peer,(burn,burn_seconds),epoch)?;}
        if !storage.enabled {return Err(Error(StatusCode::SERVICE_UNAVAILABLE,"管理员尚未启用私有对象存储，私信不会降级为明文"));}
        if mode=="e2ee" {let active:String=db.query_row("SELECT device FROM friend_keys WHERE user_id=?",[peer],|r|r.get(0)).map_err(db_error)?;if active!=check_device{return Err(Error(StatusCode::CONFLICT,"对方私信设备发生变化，请重新建立加密会话"));}}
        let existing:Option<(i64,i64,String,String,bool,i64,i64)>=db.query_row("SELECT sender,recipient,object_key,content_hash,ready,expires,created_at FROM friend_messages WHERE id=?",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(db_error)?;
        if let Some((sender,recipient,key,old,ready,expires,created))=existing {
            if sender!=s.user.id||recipient!=peer||old!=hash||expires<=now()||(!ready&&created<=now()-3600){return Err(Error(StatusCode::CONFLICT,"消息编号已使用或消息已过期"));}
            return Ok((s.user.id,key,ready,storage));
        }
        let (total,own,recent):(i64,i64,i64)=db.query_row("SELECT count(*),sum(sender=?),sum(sender=? AND created_at>?) FROM friend_messages",params![s.user.id,s.user.id,now()-60],|r|Ok((r.get(0)?,r.get::<_,Option<i64>>(1)?.unwrap_or(0),r.get::<_,Option<i64>>(2)?.unwrap_or(0)))).map_err(db_error)?;
        if total>=10000||own>=1000||recent>=20{return Err(Error(StatusCode::TOO_MANY_REQUESTS,"临时消息容量或发送频率达到上限"));}
        let key=token()?;
        let days=storage.retention_days.max(1);
        db.execute("INSERT INTO friend_messages(id,sender,recipient,object_key,content_hash,created_at,expires,ready,burn,mode,mode_epoch,burn_seconds) VALUES(?,?,?,?,?,?,?,0,?,?,?,?)",params![id,s.user.id,peer,key,hash,now(),now()+i64::from(days)*86400,burn,mode,epoch,burn_seconds]).map_err(db_error)?;
        Ok((s.user.id,key,false,storage))
    }).await?;
    if !duplicate {
        let id = body.id.clone();
        let context = crate::friend_content::object_context(peer, &body.mode, epoch);
        let sealed = app
            .work(move |a| {
                Ok(a.vault.seal(
                    owner,
                    &format!("friend-message:{id}"),
                    &context,
                    ciphertext.as_bytes(),
                )?)
            })
            .await?;
        storage::object(&storage, "PUT", &key, sealed)
            .await
            .map_err(|e| {
                Error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    storage::failure_message(&e),
                )
            })?;
    }
    app.work(move|a| {
        let s=current(a,&headers)?;let mut connection=a.db.connection.lock().unwrap();let db=connection.transaction().map_err(db_error)?;session_valid(&db,&s)?;if body.mode=="e2ee" {device(&db,&s,&headers)?;}accepted(&db,s.user.id,peer)?;
        require_mode(&db,s.user.id,peer,&body.mode,epoch)?;
        if body.mode=="e2ee" {let active:String=db.query_row("SELECT device FROM friend_keys WHERE user_id=?",[peer],|r|r.get(0)).map_err(db_error)?;if active!=target_device{return Err(Error(StatusCode::CONFLICT,"对方私信设备已变化，消息未送达"));}}
        let ready:Option<bool>=db.query_row("SELECT ready FROM friend_messages WHERE id=? AND sender=? AND recipient=? AND expires>?",params![body.id,s.user.id,peer,now()],|r|r.get(0)).optional().map_err(db_error)?;
        let ready=ready.ok_or(Error(StatusCode::CONFLICT,"消息或好友关系已失效"))?;
        if !ready {db.execute("UPDATE friend_messages SET ready=1 WHERE id=?",[body.id]).map_err(db_error)?;db.execute("UPDATE users SET friend_revision=friend_revision+1 WHERE id=?",[peer]).map_err(db_error)?;}
        db.commit().map_err(db_error)?;
        Ok(Json(json!({"message":"加密消息已送达。"})))
    }).await
}
pub async fn content(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Api<Json<Value>> {
    let _permit = app
        .friend_objects
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "临时存储正忙，请稍后重试"))?;
    let h = headers.clone();
    let mid = id.clone();
    let (sender,recipient,key,mode,epoch)=app.work(move|a| {
        let s=current(a,&h)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let row:Option<(i64,i64,String,String,i64)>=db.query_row("SELECT sender,recipient,object_key,mode,mode_epoch FROM friend_messages WHERE id=? AND (sender=? OR recipient=?) AND ready=1 AND expires>?",params![mid,s.user.id,s.user.id,now()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(db_error)?;
        let (sender,recipient,key,mode,epoch)=row.ok_or(Error(StatusCode::NOT_FOUND,"消息不存在或已过期"))?;if mode=="e2ee" {device(&db,&s,&h)?;}
        accepted(&db,s.user.id,if sender==s.user.id{recipient}else{sender})?;Ok((sender,recipient,key,mode,epoch))
    }).await?;
    let storage = app.runtime.read().unwrap().settings.storage.clone();
    let sealed = storage::object(&storage, "GET", &key, Vec::new())
        .await
        .map_err(|_| {
            Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "对象暂不可用或已由生命周期清理",
            )
        })?;
    app.work(move |a| {
        let s = current(a, &headers)?;
        let db = a.db.connection.lock().unwrap();
        session_valid(&db, &s)?;
        if mode == "e2ee" {
            device(&db, &s, &headers)?;
        }
        accepted(
            &db,
            s.user.id,
            if sender == s.user.id {
                recipient
            } else {
                sender
            },
        )?;
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM friend_messages WHERE id=? AND expires>?)",
                params![id, now()],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if !exists {
            return Err(Error(StatusCode::NOT_FOUND, "消息已过期"));
        }
        let plain = a.vault.open(
            sender,
            &format!("friend-message:{id}"),
            &crate::friend_content::object_context(recipient, &mode, epoch),
            &sealed,
        )?;
        let ciphertext = std::str::from_utf8(&plain).map_err(anyhow::Error::from)?;
        Ok(Json(json!({"ciphertext":ciphertext})))
    })
    .await
}
pub async fn read(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Api<Json<Value>> {
    app.work(move|a| {
        let s=current(a,&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let peer:Option<i64>=db.query_row("SELECT sender FROM friend_messages WHERE id=? AND recipient=? AND ready=1 AND (expires>? OR (burn=1 AND read_at IS NOT NULL))",params![id,s.user.id,now()],|r|r.get(0)).optional().map_err(db_error)?;
        accepted(&db,s.user.id,peer.ok_or(Error(StatusCode::NOT_FOUND,"消息不存在"))?)?;let mode:String=db.query_row("SELECT mode FROM friend_messages WHERE id=?",[&id],|r|r.get(0)).map_err(db_error)?;if mode=="e2ee" {device(&db,&s,&headers)?;}
        db.execute("UPDATE friend_messages SET read_at=COALESCE(read_at,?),expires=CASE WHEN burn=1 AND read_at IS NULL THEN min(expires,?+COALESCE(burn_seconds,0)) ELSE expires END WHERE id=? AND recipient=?",params![now(),now(),id,s.user.id]).map_err(db_error)?;
        let (read_at,expires):(i64,i64)=db.query_row("SELECT read_at,expires FROM friend_messages WHERE id=? AND recipient=?",params![id,s.user.id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
        Ok(Json(json!({"message":"已读","read_at":read_at,"expires":expires})))
    }).await
}
pub fn start_cleanup(app: Arc<App>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            // Expired data is inaccessible immediately, even when bucket deletion is unavailable.
            let rows=app.work(|a| {
                let db=a.db.connection.lock().unwrap();
                let mut q=db.prepare("SELECT id,object_key,0 AS probe,expires AS deadline FROM friend_messages WHERE expires<=? OR (ready=0 AND created_at<?) UNION ALL SELECT '',object_key,1,created_at FROM storage_probes WHERE created_at<? ORDER BY deadline LIMIT 100").map_err(db_error)?;
                q.query_map(params![now(),now()-3600,now()-300],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,bool>(2)?))).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)
            }).await;
            let Ok(rows) = rows else {
                continue;
            };
            for (id, key, probe) in rows {
                let Ok(_permit) = app.friend_objects.clone().try_acquire_owned() else {
                    break;
                };
                let storage = app.runtime.read().unwrap().settings.storage.clone();
                if storage::remove(&storage, &key).await.is_err() {
                    break;
                }
                let _=app.work(move|a| {let db=a.db.connection.lock().unwrap();if probe {db.execute("DELETE FROM storage_probes WHERE object_key=? AND created_at<?",params![key,now()-300]).map_err(db_error)?;} else {db.execute("DELETE FROM friend_messages WHERE id=? AND object_key=? AND (expires<=? OR (ready=0 AND created_at<?))",params![id,key,now(),now()-3600]).map_err(db_error)?;}Ok(())}).await;
            }
        }
    });
}
