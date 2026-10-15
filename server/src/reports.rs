//! Explicit, bounded moderation submissions. Never collect chats or decrypt E2EE.
use crate::{
    app::{Api, App, Error},
    db::now,
    preferences::session_valid,
    vault::token,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    account: i64,
    #[serde(default)]
    before: Option<i64>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    account: i64,
    kind: String,
    target: String,
    reason: String,
    request_id: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    account: i64,
    revision: i64,
    status: String,
    reply: String,
    note: String,
    request_id: String,
}
#[derive(Default, Deserialize, Serialize)]
struct Content {
    reason: String,
    reply: String,
    note: String,
}
fn missing() -> Error {
    Error(StatusCode::NOT_FOUND, "举报对象不存在或不可见")
}
fn text(value: &str, max: usize) -> bool {
    value.len() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
}
fn request_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn scope(s: &crate::db::Session, account: i64) -> Api<()> {
    if s.user.id != account {
        return Err(Error(
            StatusCode::CONFLICT,
            "账号已变化，请重新打开举报中心",
        ));
    }
    Ok(())
}
fn administrator(db: &Connection, s: &crate::db::Session) -> Api<()> {
    session_valid(db, s)?;
    let yes: bool = db
        .query_row("SELECT is_admin FROM users WHERE id=?", [s.user.id], |r| {
            r.get(0)
        })
        .map_err(anyhow::Error::from)?;
    if !yes {
        return Err(Error(StatusCode::FORBIDDEN, "仅站点管理员可处理举报"));
    }
    Ok(())
}
pub(crate) fn cleanup(db: &Connection) -> Api<()> {
    db.execute("DELETE FROM reports WHERE created_at<? OR (status IN('resolved','dismissed') AND updated_at<?)",params![now()-180*86400,now()-90*86400]).map_err(anyhow::Error::from)?;
    Ok(())
}
pub async fn create(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Draft>,
) -> Api<Json<Value>> {
    if !matches!(body.kind.as_str(), "account")
        || body.target.len() > 64
        || body.reason.trim().is_empty()
        || !text(&body.reason, 4000)
        || !request_id(&body.request_id)
    {
        return Err(Error::bad("请填写有效对象和举报原因（最多4000字节）"));
    }
    app.work(move|a|{
  let s=a.session(&headers)?;scope(&s,body.account)?;let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(anyhow::Error::from)?;session_valid(&tx,&s)?;cleanup(&tx)?;
  let content=serde_json::to_string(&body).map_err(anyhow::Error::from)?;let hash=a.vault.request_tag("report-create-v1",content.as_bytes());
  let old:Option<(String,String)>=tx.query_row("SELECT id,request_hash FROM reports WHERE owner=? AND request_id=?",params![s.user.id,body.request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(anyhow::Error::from)?;
  if let Some((id,old_hash))=old{if old_hash!=hash{return Err(Error(StatusCode::CONFLICT,"重复请求的内容已变化"));}return Ok(Json(json!({"account":s.user.id,"id":id})));}
  let peer=body.target.parse::<i64>().ok().filter(|p|*p>0&&*p<=9_007_199_254_740_991&&*p!=s.user.id).ok_or_else(missing)?;
  if peer.to_string()!=body.target{return Err(missing());}
  let allowed=tx.query_row("SELECT EXISTS(SELECT 1 FROM friendships WHERE lo=min(?,?) AND hi=max(?,?))",params![s.user.id,peer,s.user.id,peer],|r|r.get::<_,bool>(0)).map_err(anyhow::Error::from)?;
  if !allowed{return Err(missing());}
  let(total,daily,open):(i64,i64,i64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM(owner=? AND created_at>?),0),COALESCE(SUM(owner=? AND status IN('pending','processing')),0) FROM reports",params![s.user.id,now()-86400,s.user.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(anyhow::Error::from)?;
  if total>=5000||daily>=5||open>=16{return Err(Error(StatusCode::TOO_MANY_REQUESTS,"举报数量已达上限，请稍后再提交"));}
  let id=token()?;let record=Content{reason:body.reason,..Default::default()};let ciphertext=a.vault.seal(s.user.id,&id,"report-v1",&serde_json::to_vec(&record).map_err(anyhow::Error::from)?)?;
  tx.execute("INSERT INTO reports(id,owner,kind,target,request_id,request_hash,ciphertext,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)",params![id,s.user.id,body.kind,body.target,body.request_id,hash,ciphertext,now(),now()]).map_err(anyhow::Error::from)?;tx.commit().map_err(anyhow::Error::from)?;Ok(Json(json!({"account":s.user.id,"id":id})))
 }).await
}
async fn listing(app: Arc<App>, headers: HeaderMap, q: Listing, admin: bool) -> Api<Json<Value>> {
    if q.before.is_some_and(|n| n <= 0) {
        return Err(Error::bad("分页参数无效"));
    }
    app.work(move|a|{
  let s=a.session(&headers)?;scope(&s,q.account)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;if admin{administrator(&db,&s)?;}
  let records={let mut stmt=db.prepare("SELECT seq,id,owner,kind,target,status,revision,ciphertext,created_at,updated_at FROM reports WHERE (? OR owner=?) AND seq<? ORDER BY seq DESC LIMIT 51").map_err(anyhow::Error::from)?;stmt.query_map(params![admin,s.user.id,q.before.unwrap_or(i64::MAX)],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,i64>(6)?,r.get::<_,Vec<u8>>(7)?,r.get::<_,i64>(8)?,r.get::<_,i64>(9)?))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?};
  let more=records.len()>50;let mut items=Vec::new();let mut last=0;
  for(seq,id,owner,kind,target,status,revision,ciphertext,created,updated)in records.into_iter().take(50){last=seq;let content:Content=serde_json::from_slice(&a.vault.open(owner,&id,"report-v1",&ciphertext)?).map_err(anyhow::Error::from)?;let mut row=json!({"id":id,"kind":kind,"target":target,"status":status,"revision":revision,"reason":content.reason,"reply":content.reply,"created_at":created,"updated_at":updated});
   if admin{row["owner"]=json!(owner);row["note"]=json!(content.note);let mut stmt=db.prepare("SELECT actor,status,created_at FROM report_audit WHERE report_id=? ORDER BY revision DESC LIMIT 32").map_err(anyhow::Error::from)?;row["audit"]=json!(stmt.query_map([&id],|r|Ok(json!({"actor":r.get::<_,i64>(0)?,"status":r.get::<_,String>(1)?,"created_at":r.get::<_,i64>(2)?}))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?);}
   items.push(row);
  }Ok(Json(json!({"account":s.user.id,"items":items,"next":if more{Some(last)}else{None}})))
 }).await
}
pub async fn list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<Listing>,
) -> Api<Json<Value>> {
    listing(app, headers, q, false).await
}
pub async fn admin_list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<Listing>,
) -> Api<Json<Value>> {
    listing(app, headers, q, true).await
}
pub async fn decide(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Decision>,
) -> Api<Json<Value>> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(missing());
    }
    if body.revision < 0
        || !matches!(
            body.status.as_str(),
            "processing" | "resolved" | "dismissed"
        )
        || !text(&body.reply, 2000)
        || !text(&body.note, 2000)
        || !request_id(&body.request_id)
        || matches!(body.status.as_str(), "resolved" | "dismissed") && body.reply.trim().is_empty()
    {
        return Err(Error::bad("请填写有效状态及处理说明"));
    }
    app.work(move|a|{
  let s=a.session(&headers)?;scope(&s,body.account)?;let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(anyhow::Error::from)?;administrator(&tx,&s)?;
  let hash=a.vault.request_tag("report-decision-v1",serde_json::to_string(&body).map_err(anyhow::Error::from)?.as_bytes());let old:Option<String>=tx.query_row("SELECT request_hash FROM report_audit WHERE report_id=? AND request_id=? AND actor=?",params![id,body.request_id,s.user.id],|r|r.get(0)).optional().map_err(anyhow::Error::from)?;
  if let Some(old)=old{if old!=hash{return Err(Error(StatusCode::CONFLICT,"重复处理请求已变化"));}return Ok(Json(json!({"account":s.user.id,"message":"处理已保存"})));}
  let(owner,revision,ciphertext):(i64,i64,Vec<u8>)=tx.query_row("SELECT owner,revision,ciphertext FROM reports WHERE id=?",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(anyhow::Error::from)?.ok_or_else(missing)?;
  if revision!=body.revision{return Err(Error(StatusCode::CONFLICT,"此举报已由其他管理员更新，请刷新后处理"));}
  if revision>=128{return Err(Error::bad("此举报处理记录已达上限"));}
  let mut content:Content=serde_json::from_slice(&a.vault.open(owner,&id,"report-v1",&ciphertext)?).map_err(anyhow::Error::from)?;content.reply=body.reply;content.note=body.note;
  let ciphertext=a.vault.seal(owner,&id,"report-v1",&serde_json::to_vec(&content).map_err(anyhow::Error::from)?)?;
  let detail=a.vault.seal(owner,&id,&format!("report-audit-v1:{}",revision+1),&serde_json::to_vec(&content).map_err(anyhow::Error::from)?)?;
  tx.execute("UPDATE reports SET status=?,revision=revision+1,ciphertext=?,updated_at=? WHERE id=?",params![body.status,ciphertext,now(),id]).map_err(anyhow::Error::from)?;
  tx.execute("INSERT INTO report_audit(report_id,revision,actor,status,ciphertext,request_id,request_hash,created_at) VALUES(?,?,?,?,?,?,?,?)",params![id,revision+1,s.user.id,body.status,detail,body.request_id,hash,now()]).map_err(anyhow::Error::from)?;
  tx.commit().map_err(anyhow::Error::from)?;Ok(Json(json!({"account":s.user.id,"message":"处理已保存"})))
 }).await
}
