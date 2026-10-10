//! Account friendships and opaque, expiring Olm envelopes. No private keys or message text.
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
        let s=if body.password.is_empty() {let s=current(a,&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;device(&db,&s,&headers)?;let old:String=db.query_row("SELECT device FROM friend_keys WHERE user_id=?",[s.user.id],|r|r.get(0)).map_err(db_error)?;if old!=body.device{return Err(Error::bad("设备接管需要重新验证密码"));}s} else {a.reauthenticate(&headers,&Zeroizing::new(body.password))?};
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
pub async fn me(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a| {
        let s=current(a,&headers)?; let storage=a.runtime.read().unwrap().settings.storage.clone(); let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        db.execute("INSERT OR IGNORE INTO friend_keys(user_id,code,public_key) VALUES(?,?,'')",params![s.user.id,&token()?[..24]]).map_err(db_error)?;
        let (code,key):(String,String)=db.query_row("SELECT code,public_key FROM friend_keys WHERE user_id=?",[s.user.id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
        let left:i64=db.query_row("SELECT count(*) FROM friend_prekeys WHERE user_id=? AND used=0",[s.user.id],|r|r.get(0)).map_err(db_error)?;
        Ok(Json(json!({"id":s.user.id,"code":code,"public_key":key,"prekeys_left":left,"storage_enabled":storage.enabled,"retention_days":if storage.retention_days==0{7}else{storage.retention_days}})))
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
        let mut statement=db.prepare("SELECT u.id,COALESCE(json_extract(p.data,'$.display_name'),''),k.public_key,f.requester,f.status,f.blocked_by,k.device FROM friendships f JOIN users u ON u.id=CASE WHEN f.lo=? THEN f.hi ELSE f.lo END JOIN friend_keys k ON k.user_id=u.id LEFT JOIN profiles p ON p.user_id=u.id WHERE (f.lo=? OR f.hi=?) AND (f.status!=2 OR f.blocked_by=?) AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=? ORDER BY f.status,u.id LIMIT 132").map_err(db_error)?;
        let rows=statement.query_map(params![s.user.id,s.user.id,s.user.id,s.user.id,now()],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"name":r.get::<_,String>(1)?,"public_key":r.get::<_,String>(2)?,"requester":r.get::<_,i64>(3)?,"status":r.get::<_,i64>(4)?,"blocked_by":r.get::<_,i64>(5)?,"device":r.get::<_,String>(6)?}))).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)?;
        let unread:i64=db.query_row("SELECT count(*) FROM friend_messages m JOIN friendships f ON f.lo=min(m.sender,m.recipient) AND f.hi=max(m.sender,m.recipient) WHERE m.recipient=? AND m.ready=1 AND m.read_at IS NULL AND m.expires>? AND f.status=1",params![s.user.id,now()],|r|r.get(0)).map_err(db_error)?;
        Ok(Json(json!({"friends":rows,"unread":unread})))
    }).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    code: String,
}
pub async fn request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Request>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let s=current(a,&headers)?; if body.code.len()!=24 || !body.code.bytes().all(|b|b.is_ascii_hexdigit()) {return Err(Error::bad("好友码为24位字符"));}
        let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let peer:Option<i64>=db.query_row("SELECT k.user_id FROM friend_keys k JOIN users u ON u.id=k.user_id WHERE k.code=? AND length(k.public_key)>0 AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?",params![body.code.to_lowercase(),now()],|r|r.get(0)).optional().map_err(db_error)?;
        let peer=peer.filter(|&id|id!=s.user.id).ok_or(Error::bad("好友码不可用"))?;
        let own:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM friend_keys WHERE user_id=? AND length(public_key)>0)",[s.user.id],|r|r.get(0)).map_err(db_error)?;
        if !own{return Err(Error::bad("先设置自己的私信密钥"));}
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
        let s=current(a,&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?; device(&db,&s,&headers)?;accepted(&db,s.user.id,peer)?;
        let mut query=db.prepare("SELECT seq,id,sender,recipient,created_at,expires,read_at,burn FROM friend_messages WHERE ((sender=? AND recipient=?) OR (sender=? AND recipient=?)) AND ready=1 AND expires>? AND (?=0 OR seq<?) ORDER BY seq DESC LIMIT 30").map_err(db_error)?;
        let rows=query.query_map(params![s.user.id,peer,peer,s.user.id,now(),page.before,page.before],|r|Ok(json!({"seq":r.get::<_,i64>(0)?,"id":r.get::<_,String>(1)?,"sender":r.get::<_,i64>(2)?,"recipient":r.get::<_,i64>(3)?,"created_at":r.get::<_,i64>(4)?,"expires":r.get::<_,i64>(5)?,"read_at":r.get::<_,Option<i64>>(6)?,"burn":r.get::<_,bool>(7)?}))).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)?;
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
}
pub async fn send(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Send>,
) -> Api<Json<Value>> {
    if !uuid(&body.id) || !valid_ciphertext(&body.ciphertext) {
        return Err(Error::bad("只接受限制以内的Olm双棘轮密文"));
    }
    let _permit = app
        .friend_objects
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "临时存储正忙，请稍后重试"))?;
    let storage = app.runtime.read().unwrap().settings.storage.clone();
    if !storage.enabled {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "管理员尚未启用私有对象存储，私信不会降级为明文",
        ));
    }
    let check_headers = headers.clone();
    let id = body.id.clone();
    let peer = body.peer;
    let target_device = serde_json::from_str::<Value>(&body.ciphertext).unwrap()["device"]
        .as_str()
        .unwrap()
        .to_owned();
    let check_device = target_device.clone();
    let burn = body.burn;
    let hash = crate::db::digest(&format!("{}:{burn}", body.ciphertext));
    let (owner,key,duplicate,storage)=app.work(move |a| {
        let s=current(a,&check_headers)?;let runtime=a.runtime.read().unwrap();
        if !runtime.settings.storage.enabled {return Err(Error(StatusCode::SERVICE_UNAVAILABLE,"私信存储已关闭"));}
        let storage=runtime.settings.storage.clone();let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;device(&db,&s,&check_headers)?; accepted(&db,s.user.id,peer)?;
        let active:String=db.query_row("SELECT device FROM friend_keys WHERE user_id=?",[peer],|r|r.get(0)).map_err(db_error)?;if active!=check_device{return Err(Error(StatusCode::CONFLICT,"对方私信设备发生变化，请重新建立加密会话"));}
        let existing:Option<(i64,i64,String,String,bool,i64,i64)>=db.query_row("SELECT sender,recipient,object_key,content_hash,ready,expires,created_at FROM friend_messages WHERE id=?",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(db_error)?;
        if let Some((sender,recipient,key,old,ready,expires,created))=existing {
            if sender!=s.user.id||recipient!=peer||old!=hash||expires<=now()||(!ready&&created<=now()-3600){return Err(Error(StatusCode::CONFLICT,"消息编号已使用或消息已过期"));}
            return Ok((s.user.id,key,ready,storage));
        }
        let (total,own,recent):(i64,i64,i64)=db.query_row("SELECT count(*),sum(sender=?),sum(sender=? AND created_at>?) FROM friend_messages",params![s.user.id,s.user.id,now()-60],|r|Ok((r.get(0)?,r.get::<_,Option<i64>>(1)?.unwrap_or(0),r.get::<_,Option<i64>>(2)?.unwrap_or(0)))).map_err(db_error)?;
        if total>=10000||own>=1000||recent>=20{return Err(Error(StatusCode::TOO_MANY_REQUESTS,"临时消息容量或发送频率达到上限"));}
        let key=token()?;
        let days=storage.retention_days.max(1);
        db.execute("INSERT INTO friend_messages(id,sender,recipient,object_key,content_hash,created_at,expires,ready,burn) VALUES(?,?,?,?,?,?,?,0,?)",params![id,s.user.id,peer,key,hash,now(),now()+i64::from(days)*86400,burn]).map_err(db_error)?;
        Ok((s.user.id,key,false,storage))
    }).await?;
    if !duplicate {
        let id = body.id.clone();
        let ciphertext = Zeroizing::new(body.ciphertext);
        let sealed = app
            .work(move |a| {
                Ok(a.vault.seal(
                    owner,
                    &format!("friend-message:{id}"),
                    &peer.to_string(),
                    ciphertext.as_bytes(),
                )?)
            })
            .await?;
        storage::object(&storage, "PUT", &key, sealed)
            .await
            .map_err(|_| {
                Error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "对象存储写入失败，消息未送达；请稍后重试",
                )
            })?;
    }
    app.work(move|a| {
        let s=current(a,&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;device(&db,&s,&headers)?;accepted(&db,s.user.id,peer)?;
        let active:String=db.query_row("SELECT device FROM friend_keys WHERE user_id=?",[peer],|r|r.get(0)).map_err(db_error)?;if active!=target_device{return Err(Error(StatusCode::CONFLICT,"对方私信设备已变化，消息未送达"));}
        let changed=db.execute("UPDATE friend_messages SET ready=1 WHERE id=? AND sender=? AND recipient=? AND expires>?",params![body.id,s.user.id,peer,now()]).map_err(db_error)?;
        if changed!=1{return Err(Error(StatusCode::CONFLICT,"消息或好友关系已失效"));}
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
    let (sender,recipient,key)=app.work(move|a| {
        let s=current(a,&h)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;device(&db,&s,&h)?;
        let row:Option<(i64,i64,String)>=db.query_row("SELECT sender,recipient,object_key FROM friend_messages WHERE id=? AND (sender=? OR recipient=?) AND ready=1 AND expires>?",params![mid,s.user.id,s.user.id,now()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
        let (sender,recipient,key)=row.ok_or(Error(StatusCode::NOT_FOUND,"消息不存在或已过期"))?;
        accepted(&db,s.user.id,if sender==s.user.id{recipient}else{sender})?;Ok((sender,recipient,key))
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
        device(&db, &s, &headers)?;
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
            &recipient.to_string(),
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
        let s=current(a,&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;device(&db,&s,&headers)?;
        let peer:Option<i64>=db.query_row("SELECT sender FROM friend_messages WHERE id=? AND recipient=? AND ready=1 AND (expires>? OR (burn=1 AND read_at IS NOT NULL))",params![id,s.user.id,now()],|r|r.get(0)).optional().map_err(db_error)?;
        accepted(&db,s.user.id,peer.ok_or(Error(StatusCode::NOT_FOUND,"消息不存在"))?)?;
        db.execute("UPDATE friend_messages SET read_at=COALESCE(read_at,?),expires=CASE WHEN burn=1 THEN min(expires,?) ELSE expires END WHERE id=? AND recipient=?",params![now(),now(),id,s.user.id]).map_err(db_error)?;
        Ok(Json(json!({"message":"已读"})))
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
                let mut q=db.prepare("SELECT id,object_key FROM friend_messages WHERE expires<=? OR (ready=0 AND created_at<?) ORDER BY expires LIMIT 100").map_err(db_error)?;
                q.query_map(params![now(),now()-3600],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)
            }).await;
            let Ok(rows) = rows else {
                continue;
            };
            for (id, key) in rows {
                let Ok(_permit) = app.friend_objects.clone().try_acquire_owned() else {
                    break;
                };
                let storage = app.runtime.read().unwrap().settings.storage.clone();
                if storage::object(&storage, "DELETE", &key, Vec::new())
                    .await
                    .is_err()
                {
                    break;
                }
                let _=app.work(move|a| {a.db.connection.lock().unwrap().execute("DELETE FROM friend_messages WHERE id=? AND object_key=? AND (expires<=? OR (ready=0 AND created_at<?))",params![id,key,now(),now()-3600]).map_err(db_error)?;Ok(())}).await;
            }
        }
    });
}
