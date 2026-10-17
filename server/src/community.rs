//! Member-visible community content. Payloads live only in encrypted S3 objects.
#[path = "community_changes.rs"]
mod changes;
#[path = "community_replies.rs"]
mod replies;
use crate::{
    app::{Api, App, Error},
    db::{Session, now},
    preferences::session_valid,
    storage,
    vault::token,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
pub use changes::{control, edit, icon, set_icon};
pub(crate) use replies::reply_summary;
pub use replies::{reply_list, reply_read};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use zeroize::Zeroizing;

#[derive(Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Payload {
    pub title: String,
    pub text: String,
    pub image: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    account: i64,
    id: String,
    kind: String,
    parent: Option<String>,
    issued_at: i64,
    payload: Payload,
    #[serde(default = "general")]
    category: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    author: Option<String>,
    #[serde(default)]
    latest: bool,
    account: i64,
    before: Option<i64>,
    parent: Option<String>,
    #[serde(default = "post")]
    kind: String,
    category: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvatarQuery {
    account: i64,
    v: String,
}
fn author(db: &Connection, owner: i64) -> Api<(String, String)> {
    let profile: Option<(Option<String>, String)> = db
        .query_row(
            "SELECT json_extract(data,'$.display_name'),avatar_hash FROM profiles WHERE user_id=?",
            [owner],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(anyhow::Error::from)?;
    let (name, hash) = profile.unwrap_or_default();
    Ok((
        name.filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("WebTS 用户 {owner}")),
        hash,
    ))
}
pub async fn avatar(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<AvatarQuery>,
) -> Api<Response> {
    if q.v.len() != 64
        || !q
            .v
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::bad("头像版本无效"));
    }
    app.work(move |a| {
        let s = a.session(&headers)?;
        scope(&s, q.account)?;
        let runtime = a.runtime.read().unwrap();
        let db = a.db.connection.lock().unwrap();
        allow_read(&runtime.settings, &db, &s)?;
        let item = visible(&db, &s, &id)?;
        let data: Option<(String,String)> = db.query_row(
            "SELECT json_extract(data,'$.avatar'),avatar_hash FROM profiles WHERE user_id=? AND avatar_hash!=''",
            [item.owner], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional().map_err(anyhow::Error::from)?;
        let (encoded, hash) = data.ok_or_else(missing)?;
        if hash != q.v { return Err(Error(StatusCode::CONFLICT,"头像已更新，请刷新社区")); }
        // Saved avatars are sanitized PNG; the global upload ceiling preserves older
        // valid avatars when the operator later lowers the limit for new uploads.
        if encoded.len() > (128usize * 1024).div_ceil(3) * 4 { return Err(missing()); }
        let bytes = STANDARD.decode(encoded).map_err(|_| missing())?;
        if bytes.len() > 128 * 1024 || image::guess_format(&bytes).ok() != Some(image::ImageFormat::Png) { return Err(missing()); }
        Ok(([("content-type","image/png"),("cache-control","private, no-store"),("x-content-type-options","nosniff")],bytes).into_response())
    }).await
}
fn general() -> String {
    "general".into()
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    id: String,
    name: String,
    enabled: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    icon: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Categories {
    account: i64,
    revision: i64,
    password: String,
    categories: Vec<Category>,
}
fn categories(db: &Connection) -> Api<Vec<Category>> {
    let mut stmt = db
        .prepare("SELECT id,name,enabled,COALESCE((SELECT version FROM community_icons WHERE category=community_categories.id),'') FROM community_categories ORDER BY position,id")
        .map_err(anyhow::Error::from)?;
    Ok(stmt
        .query_map([], |r| {
            Ok(Category {
                id: r.get(0)?,
                name: r.get(1)?,
                enabled: r.get(2)?,
                icon: r.get(3)?,
            })
        })
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(anyhow::Error::from)?)
}
fn post() -> String {
    "post".into()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    account: i64,
    #[serde(default)]
    liked: bool,
    #[serde(default)]
    remove: bool,
    #[serde(default)]
    reviewed_revision: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configure {
    enabled: bool,
    password: String,
}
fn missing() -> Error {
    Error(StatusCode::NOT_FOUND, "内容不存在或不可见")
}
fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn scope(s: &Session, account: i64) -> Api<()> {
    if s.user.id != account {
        Err(Error(StatusCode::CONFLICT, "账号已变化，请重新打开社区"))
    } else {
        Ok(())
    }
}
fn administrator(db: &Connection, s: &Session) -> Api<bool> {
    session_valid(db, s)?;
    if s.user.public_device {
        return Ok(false);
    }
    Ok(db
        .query_row("SELECT is_admin FROM users WHERE id=?", [s.user.id], |r| {
            r.get(0)
        })
        .map_err(anyhow::Error::from)?)
}
fn allow_read(settings: &crate::settings::Settings, db: &Connection, s: &Session) -> Api<()> {
    session_valid(db, s)?;
    if settings.community_enabled || administrator(db, s)? {
        Ok(())
    } else {
        Err(Error(StatusCode::FORBIDDEN, "社区动态当前关闭"))
    }
}

struct Item {
    owner: i64,
    kind: String,
    parent: Option<String>,
    key: String,
    store: String,
    version: String,
}
fn visible(db: &Connection, s: &Session, id: &str) -> Api<Item> {
    if !uuid(id) {
        return Err(missing());
    }
    let admin = administrator(db, s)?;
    let row:Option<Item>=db.query_row("SELECT c.owner,c.kind,c.parent,c.object_key,c.store_id,c.content_version FROM community_items c JOIN users u ON u.id=c.owner WHERE c.id=? AND c.ready=1 AND c.deleted=0 AND (? OR (u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=? AND NOT EXISTS(SELECT 1 FROM friendships f WHERE f.lo=min(c.owner,?) AND f.hi=max(c.owner,?) AND f.status=2)))",params![id,admin,now(),s.user.id,s.user.id],|r|Ok(Item{owner:r.get(0)?,kind:r.get(1)?,parent:r.get(2)?,key:r.get(3)?,store:r.get(4)?,version:r.get(5)?})).optional().map_err(anyhow::Error::from)?;
    let row = row.ok_or_else(missing)?;
    if row.kind == "report" && !admin && row.owner != s.user.id {
        return Err(missing());
    }
    if row.kind == "comment" {
        let parent = row.parent.as_ref().ok_or_else(missing)?;
        let active:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM community_items WHERE id=? AND kind='post' AND ready=1 AND deleted=0)",[parent],|r|r.get(0)).map_err(anyhow::Error::from)?;
        if !active {
            return Err(missing());
        }
        // Parent visibility is also checked, including blocks and bans.
        let root = visible(db, s, parent)?;
        if root.kind != "post" {
            return Err(missing());
        }
    }
    Ok(row)
}
fn validate(body: &mut Draft) -> Api<()> {
    let text = |s: &str, max| {
        s.len() <= max
            && !s
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    };
    if !uuid(&body.id)
        || !matches!(body.kind.as_str(), "post" | "comment" | "report")
        || !text(&body.payload.title, 160)
        || !text(&body.payload.text, 4000)
        || body.payload.image.len() > 180000
        || body.parent.as_deref().is_some_and(|v| !uuid(v))
        || body.category != "general" && (!uuid(&body.category) || body.kind != "post")
    {
        return Err(Error::bad(
            "内容格式无效；标题最多160字节，正文4000字节，图片128KiB",
        ));
    }
    if body.kind == "post" {
        if body.parent.is_some() {
            return Err(Error::bad("帖子不能指定父内容"));
        }
        crate::site::image(&mut body.payload.image)?;
    } else if body.parent.is_none()
        || !body.payload.title.is_empty()
        || !body.payload.image.is_empty()
    {
        return Err(Error::bad("评论和举报只接受文字及目标编号"));
    }
    if body.payload.text.trim().is_empty()
        && body.payload.title.trim().is_empty()
        && body.payload.image.is_empty()
    {
        return Err(Error::bad("请填写内容"));
    }
    Ok(())
}
struct Write {
    key: String,
    config: storage::Storage,
    ready: bool,
    sealed: Vec<u8>,
}
fn prepare(a: &App, headers: &HeaderMap, body: &mut Draft) -> Api<Write> {
    let s = a.session(headers)?;
    scope(&s, body.account)?;
    validate(body)?;
    let plain = Zeroizing::new(serde_json::to_vec(&body.payload).map_err(anyhow::Error::from)?);
    let binding = Zeroizing::new(
        serde_json::to_vec(&(&body.kind, &body.parent, body.issued_at, &body.payload))
            .map_err(anyhow::Error::from)?,
    );
    // Preserve exact retries from clients predating categories, including pending uploads.
    let hash = if body.category == "general" {
        a.vault.request_tag("community-request-v1", &binding)
    } else {
        a.vault.request_tag(
            "community-request-v2",
            &Zeroizing::new(
                serde_json::to_vec(&(
                    &body.category,
                    &body.kind,
                    &body.parent,
                    body.issued_at,
                    &body.payload,
                ))
                .map_err(anyhow::Error::from)?,
            ),
        )
    };
    let runtime = a.runtime.read().unwrap();
    if !runtime.settings.community_enabled {
        return Err(Error(StatusCode::FORBIDDEN, "社区动态当前关闭"));
    }
    let db = a.db.connection.lock().unwrap();
    session_valid(&db, &s)?;
    if body.kind == "comment" {
        changes::comment_allowed(&db, &s, body.parent.as_deref().ok_or_else(missing)?)?;
    }
    let old:Option<(i64,String,String,String,bool,bool)>=db.query_row("SELECT owner,request_hash,object_key,store_id,ready,deleted FROM community_items WHERE id=?",[&body.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(anyhow::Error::from)?;
    if let Some((owner, previous, key, store, ready, deleted)) = old {
        if owner != s.user.id || hash != previous || deleted {
            return Err(Error(
                StatusCode::CONFLICT,
                "内容编号已使用或已删除，请重新发起",
            ));
        }
        return Ok(Write {
            key,
            config: storage::resolve(&runtime.settings, &store)?,
            ready,
            sealed: a.vault.seal(owner, &body.id, "community-v1", &plain)?,
        });
    }
    if body.issued_at < now() - 3600 || body.issued_at > now() + 300 {
        return Err(Error::bad("发送时间已过期或设备时钟有误，请刷新后重试"));
    }
    if let Some(parent) = &body.parent {
        let target = visible(&db, &s, parent)?;
        if target.kind == "report" || body.kind == "comment" && target.kind != "post" {
            return Err(missing());
        }
    }
    if body.kind == "post"
        && !db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM community_categories WHERE id=? AND enabled=1)",
                [&body.category],
                |r| r.get::<_, bool>(0),
            )
            .map_err(anyhow::Error::from)?
    {
        return Err(Error::bad("分区不存在或已停止接收新帖，请刷新后选择"));
    }
    let (total,daily):(i64,i64)=db.query_row("SELECT (SELECT count(*) FROM community_items),(SELECT count(*) FROM community_items WHERE owner=? AND kind=? AND created_at>?)",params![s.user.id,body.kind,now()-86400],|r|Ok((r.get(0)?,r.get(1)?))).map_err(anyhow::Error::from)?;
    let cap = match body.kind.as_str() {
        "report" => 5,
        "comment" => 100,
        _ => 20,
    };
    if total >= 30000 || daily >= cap {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "社区容量或今日发布数量已达上限",
        ));
    }
    let (store, config) = storage::selected(&runtime.settings)?;
    if !config.enabled {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "管理员尚未开启对象存储，内容不会保存到本地",
        ));
    }
    let key = token()?;
    db.execute("INSERT INTO community_items(id,owner,kind,parent,object_key,store_id,request_hash,created_at,category) VALUES(?,?,?,?,?,?,?,?,?)",params![body.id,s.user.id,body.kind,body.parent,key,store,hash,now(),body.category]).map_err(anyhow::Error::from)?;
    Ok(Write {
        key,
        config,
        ready: false,
        sealed: a.vault.seal(s.user.id, &body.id, "community-v1", &plain)?,
    })
}
fn finish(a: &App, headers: &HeaderMap, body: &Draft) -> Api<Json<Value>> {
    let s = a.session(headers)?;
    scope(&s, body.account)?;
    let runtime = a.runtime.read().unwrap();
    if !runtime.settings.community_enabled {
        return Err(Error(StatusCode::FORBIDDEN, "社区动态当前关闭"));
    }
    let db = a.db.connection.lock().unwrap();
    session_valid(&db, &s)?;
    if let Some(parent) = &body.parent {
        let _ = visible(&db, &s, parent)?;
        if body.kind == "comment" {
            changes::comment_allowed(&db, &s, parent)?;
        }
    }
    let n = db
        .execute(
            "UPDATE community_items SET ready=1 WHERE id=? AND owner=? AND deleted=0",
            params![body.id, s.user.id],
        )
        .map_err(anyhow::Error::from)?;
    if n != 1 {
        return Err(missing());
    }
    Ok(Json(json!({"account":s.user.id,"id":body.id})))
}
pub async fn publish(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Draft>,
) -> Api<Json<Value>> {
    if !uuid(&body.id) {
        return Err(Error::bad("内容编号无效"));
    }
    let permit = app.object_permit(&headers).await?;
    let transfer = app
        .friend_transfer(&format!("community:{}", body.id))
        .try_lock_owned()
        .map_err(|_| Error(StatusCode::CONFLICT, "此内容正在传输或清理，请稍后重试"))?;
    tokio::spawn(async move {
        let _permit = permit;
        let h = headers.clone();
        let (body, write) = app
            .expensive(move |a| {
                let write = prepare(a, &h, &mut body)?;
                Ok((body, write))
            })
            .await?;
        if !write.ready {
            storage::community_object(&write.config, "PUT", &write.key, write.sealed)
                .await
                .map_err(|e| {
                    Error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        storage::failure_message(&e),
                    )
                })?;
        }
        app.work(move |a| {
            let _lease = transfer;
            finish(a, &headers, &body)
        })
        .await
    })
    .await
    .map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "内容传输中断，请使用原编号重试",
        )
    })?
}
pub async fn list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<Listing>,
) -> Api<Json<Value>> {
    if q.author
        .as_deref()
        .is_some_and(|id| q.kind != "post" || !uuid(id))
        || !matches!(q.kind.as_str(), "post" | "comment" | "report")
        || q.latest && q.kind != "post"
        || q.before.is_some_and(|n| n <= 0)
        || q.category
            .as_deref()
            .is_some_and(|id| q.kind != "post" || id != "general" && !uuid(id))
    {
        return Err(Error::bad("分页参数无效"));
    }
    app.work(move|a|{let s=a.session(&headers)?;scope(&s,q.account)?;let runtime=a.runtime.read().unwrap();let active=runtime.settings.community_enabled;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
  if !active && !(q.kind=="report" && administrator(&db,&s)?){return Ok(Json(json!({"account":s.user.id,"enabled":false,"items":[],"next":null,"server_time":now()})));}
  if q.kind=="report"&&!administrator(&db,&s)?{return Err(Error(StatusCode::FORBIDDEN,"仅管理员可查看举报队列"));}
  if q.kind=="comment"{let parent=q.parent.as_deref().ok_or_else(missing)?;if visible(&db,&s,parent)?.kind!="post"{return Err(missing());}}
  else if q.parent.is_some(){return Err(Error::bad("分页目标无效"));}
  // Author history is anchored to content currently visible to the reader, not an account directory.
  let author_id=q.author.as_deref().map(|id|visible(&db,&s,id).map(|r|r.owner)).transpose()?;
  let selected_author=author_id.map(|owner|author(&db,owner).map(|(name,avatar_hash)|json!({"owner":owner,"name":name,"avatar_hash":avatar_hash}))).transpose()?;
  let latest=q.latest||author_id.is_some();
  let mut stmt=db.prepare("SELECT seq,id FROM community_items WHERE kind=?1 AND parent IS ?2 AND seq<?3 AND (?4 IS NULL OR category=?4) AND ready=1 AND deleted=0 AND (?5 OR kind!='post' OR pinned=0) AND (?6 IS NULL OR owner=?6) ORDER BY seq DESC LIMIT 13").map_err(anyhow::Error::from)?;
  if let Some(category)=&q.category {if !categories(&db)?.iter().any(|c| &c.id==category){return Err(Error::bad("分区不存在，请刷新"));}stmt=db.prepare("SELECT seq,id FROM community_items WHERE kind=?1 AND parent IS ?2 AND seq<?3 AND category=?4 AND ready=1 AND deleted=0 AND (?5 OR kind!='post' OR pinned=0) AND (?6 IS NULL OR owner=?6) ORDER BY seq DESC LIMIT 13").map_err(anyhow::Error::from)?;}
  // Reports have a target parent, so their moderation queue ignores the parent filter.
  if q.kind=="report"{stmt=db.prepare("SELECT seq,id FROM community_items WHERE kind=?1 AND ?2 IS NULL AND seq<?3 AND ?4 IS NULL AND ready=1 AND deleted=0 AND (?5 OR kind!='post' OR pinned=0) AND (?6 IS NULL OR owner=?6) ORDER BY seq DESC LIMIT 13").map_err(anyhow::Error::from)?;}
  if author_id.is_some(){stmt=db.prepare("SELECT seq,id FROM community_items WHERE owner=?6 AND kind=?1 AND parent IS ?2 AND seq<?3 AND (?4 IS NULL OR category=?4) AND ?5 AND ready=1 AND deleted=0 ORDER BY seq DESC LIMIT 13").map_err(anyhow::Error::from)?;}
  let rows=stmt.query_map(params![q.kind,q.parent,q.before.unwrap_or(i64::MAX),q.category,latest,author_id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
  let more=rows.len()>12;let mut items=Vec::new();let last=rows.iter().take(12).next_back().map(|r|r.0).unwrap_or(0);
  let mut display=Vec::new();
  if q.kind=="post" && !latest && q.before.is_none(){let mut pins=db.prepare("SELECT seq,id FROM community_items WHERE kind='post' AND pinned=1 AND ready=1 AND deleted=0 AND (?1 IS NULL OR category=?1) ORDER BY seq DESC LIMIT 5").map_err(anyhow::Error::from)?;display=pins.query_map(params![q.category],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;}
  display.extend(rows.into_iter().take(12));
  for(_seq,id)in display{let row=match visible(&db,&s,&id){Ok(row)=>row,Err(Error(StatusCode::NOT_FOUND,_))=>continue,Err(e)=>return Err(e)};
   let (name,avatar_hash)=author(&db,row.owner)?;
   let liked:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM community_likes WHERE item=? AND owner=?)",params![id,s.user.id],|r|r.get(0)).map_err(anyhow::Error::from)?;let likes:i64=db.query_row("SELECT count(*) FROM community_likes WHERE item=?",[&id],|r|r.get(0)).map_err(anyhow::Error::from)?;
   items.push(json!({"id":id,"owner":row.owner,"name":name,"avatar_hash":avatar_hash,"kind":row.kind,"parent":row.parent,"created_at":db.query_row("SELECT created_at FROM community_items WHERE id=?",[&id],|r|r.get::<_,i64>(0)).map_err(anyhow::Error::from)?,"liked":liked,"likes":likes,"category":db.query_row("SELECT category FROM community_items WHERE id=?",[&id],|r|r.get::<_,String>(0)).map_err(anyhow::Error::from)?,"can_delete":row.owner==s.user.id||administrator(&db,&s)?,"can_moderate":administrator(&db,&s)?,"comments":comment_count(&db,&s,&id)?,"state":changes::state(&db,&id)?}));
  }Ok(Json(json!({"account":s.user.id,"enabled":true,"author":selected_author,"categories":categories(&db)?,"items":items,"next":if more{Some(last)}else{None},"server_time":now()})))
 }).await
}
fn comment_count(db: &Connection, s: &Session, id: &str) -> Api<i64> {
    // Count only readable authors, respecting the same block/ban boundary as comments.
    let admin = administrator(db, s)?;
    db.query_row("SELECT count(*) FROM community_items c JOIN users u ON u.id=c.owner WHERE c.parent=? AND c.kind='comment' AND c.ready=1 AND c.deleted=0 AND (? OR (u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=? AND NOT EXISTS(SELECT 1 FROM friendships f WHERE f.lo=min(c.owner,?) AND f.hi=max(c.owner,?) AND f.status=2)))",params![id,admin,now(),s.user.id,s.user.id],|r|r.get(0)).map_err(|e|anyhow::Error::from(e).into())
}
pub async fn content(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<Listing>,
) -> Api<Json<Value>> {
    let _permit = app.object_permit(&headers).await?;
    let h = headers.clone();
    let mid = id.clone();
    let (write, config) = app
        .work(move |a| {
            let s = a.session(&h)?;
            scope(&s, q.account)?;
            let runtime = a.runtime.read().unwrap();
            let db = a.db.connection.lock().unwrap();
            allow_read(&runtime.settings, &db, &s)?;
            let row = visible(&db, &s, &mid)?;
            let config = storage::resolve(&runtime.settings, &row.store)?;
            Ok((row, config))
        })
        .await?;
    let bytes = storage::community_object(&config, "GET", &write.key, vec![])
        .await
        .map_err(|_| {
            Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "内容暂不可读取，请检查原存储配置",
            )
        })?;
    app.work(move |a| read_version(a, &headers, &id, &bytes, Some(&write.key)))
        .await
}
#[cfg(test)]
fn read_complete(a: &App, headers: &HeaderMap, id: &str, bytes: &[u8]) -> Api<Json<Value>> {
    read_version(a, headers, id, bytes, None)
}
fn read_version(
    a: &App,
    headers: &HeaderMap,
    id: &str,
    bytes: &[u8],
    expected: Option<&str>,
) -> Api<Json<Value>> {
    let s = a.session(headers)?;
    let runtime = a.runtime.read().unwrap();
    let db = a.db.connection.lock().unwrap();
    allow_read(&runtime.settings, &db, &s)?;
    let row = visible(&db, &s, id)?;
    if expected.is_some_and(|key| key != row.key) {
        return Err(Error(StatusCode::CONFLICT, "帖子已更新，请刷新后重试"));
    }
    let domain = if row.version.is_empty() {
        "community-v1".into()
    } else {
        format!("community-v2:{}", row.version)
    };
    let plain = a.vault.open(row.owner, id, &domain, bytes)?;
    let payload: Payload = serde_json::from_slice(&plain).map_err(anyhow::Error::from)?;
    let liked: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM community_likes WHERE item=? AND owner=?)",
            params![id, s.user.id],
            |r| r.get(0),
        )
        .map_err(anyhow::Error::from)?;
    let likes: i64 = db
        .query_row(
            "SELECT count(*) FROM community_likes WHERE item=?",
            [id],
            |r| r.get(0),
        )
        .map_err(anyhow::Error::from)?;
    let (name, avatar_hash) = author(&db, row.owner)?;
    Ok(Json(
        json!({"account":s.user.id,"id":id,"content":payload,"liked":liked,"likes":likes,"name":name,"avatar_hash":avatar_hash,"comments":comment_count(&db,&s,id)?,"state":changes::state(&db,id)?}),
    ))
}

fn mark_deleted(db: &Connection, id: &str, actor: i64) -> Api<()> {
    db.execute("UPDATE community_items SET deleted=1,deleted_at=?,deleted_by=? WHERE (id=? OR (parent=? AND kind='comment')) AND deleted=0",params![now(),actor,id,id]).map_err(anyhow::Error::from)?;
    Ok(())
}
pub async fn delete(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let s = a.session(&headers)?;
        scope(&s, body.account)?;
        let db = a.db.connection.lock().unwrap();
        let admin = administrator(&db, &s)?;
        let owner: Option<i64> = db
            .query_row(
                "SELECT owner FROM community_items WHERE id=? AND kind!='report'",
                [&id],
                |r| r.get(0),
            )
            .optional()
            .map_err(anyhow::Error::from)?;
        if owner.is_none() || !admin && owner != Some(s.user.id) {
            return Err(missing());
        }
        mark_deleted(&db, &id, s.user.id)?;
        Ok(Json(json!({"account":s.user.id})))
    })
    .await
}
pub async fn like(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let s = a.session(&headers)?;
        scope(&s, body.account)?;
        let runtime = a.runtime.read().unwrap();
        if !runtime.settings.community_enabled {
            return Err(Error(StatusCode::FORBIDDEN, "社区动态当前关闭"));
        }
        let db = a.db.connection.lock().unwrap();
        if visible(&db, &s, &id)?.kind == "report" {
            return Err(missing());
        }
        if body.liked {
            let (total, own): (i64, i64) = db
                .query_row(
                    "SELECT (SELECT count(*) FROM community_likes),(SELECT count(*) FROM community_likes WHERE owner=?)",
                    [s.user.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(anyhow::Error::from)?;
            let exists: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM community_likes WHERE item=? AND owner=?)",
                    params![id, s.user.id],
                    |r| r.get(0),
                )
                .map_err(anyhow::Error::from)?;
            if !exists && (total >= 200000 || own >= 2000) {
                return Err(Error(StatusCode::TOO_MANY_REQUESTS, "点赞数量已达上限"));
            }
            db.execute(
                "INSERT OR IGNORE INTO community_likes VALUES(?,?)",
                params![id, s.user.id],
            )
            .map_err(anyhow::Error::from)?;
        } else {
            db.execute(
                "DELETE FROM community_likes WHERE item=? AND owner=?",
                params![id, s.user.id],
            )
            .map_err(anyhow::Error::from)?;
        }
        Ok(Json(json!({"account":s.user.id})))
    })
    .await
}
pub async fn admin_config(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a| {
        let session=crate::settings::admin(a, &headers)?;
        let runtime=a.runtime.read().unwrap();
        let db=a.db.connection.lock().unwrap();
        if !administrator(&db,&session)? {return Err(Error(StatusCode::FORBIDDEN,"管理员权限已撤销"));}
        let (total,posts,comments,reports,uploads,cleanup,retries):(i64,i64,i64,i64,i64,i64,i64)=db.query_row(
            "SELECT count(*),COALESCE(sum(kind='post' AND ready=1 AND deleted=0),0),COALESCE(sum(kind='comment' AND ready=1 AND deleted=0),0),COALESCE(sum(kind='report' AND ready=1 AND deleted=0),0),COALESCE(sum(ready=0 AND deleted=0 AND cleaned=0 AND created_at>?),0),COALESCE(sum(cleaned=0 AND (deleted=1 OR (ready=0 AND created_at<=?))),0),COALESCE(sum(cleaned=0 AND attempts>0 AND (deleted=1 OR (ready=0 AND created_at<=?))),0) FROM community_items",
            params![now()-3600,now()-3600,now()-3600],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(anyhow::Error::from)?;
        let (edits,edit_uploads,edit_cleanup,edit_retries,icons):(i64,i64,i64,i64,i64)=db.query_row("SELECT count(*),COALESCE(sum(applied=0 AND cleaned=0 AND created_at>?),0),COALESCE(sum(cleaned=0 AND (applied=1 OR created_at<=?)),0),COALESCE(sum(cleaned=0 AND attempts>0 AND (applied=1 OR created_at<=?)),0),(SELECT count(*) FROM community_icons) FROM community_changes",params![now()-3600,now()-3600,now()-3600],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(anyhow::Error::from)?;
        Ok(Json(json!({"account":session.user.id,"enabled":runtime.settings.community_enabled,"categories":categories(&db)?,"category_revision":db.query_row("SELECT revision FROM community_category_revision WHERE id=1",[],|r|r.get::<_,i64>(0)).map_err(anyhow::Error::from)?,"stats":{"total":total,"limit":30000,"posts":posts,"comments":comments,"reports":reports,"uploads":uploads+edit_uploads,"cleanup":cleanup+edit_cleanup,"retries":retries+edit_retries,"edits":edits,"icons":icons,"storage_configured":storage::selected(&runtime.settings).is_ok_and(|(_,s)|s.enabled)}})))
    }).await
}
pub async fn configure(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Configure>,
) -> Api<Json<Value>> {
    if body.password.len() > 128 {
        return Err(Error::bad("密码过长"));
    }
    app.expensive(move |a| {
        crate::settings::admin(a, &headers)?;
        let s = a.reauthenticate(&headers, &Zeroizing::new(body.password))?;
        let mut runtime = a.runtime.write().unwrap();
        let mut settings = runtime.settings.clone();
        settings.community_enabled = body.enabled;
        let bytes = Zeroizing::new(serde_json::to_vec(&settings).map_err(anyhow::Error::from)?);
        a.db.save_settings(
            s.user.id,
            &s.hash,
            &a.vault.seal(0, "site-settings", "v1", &bytes)?,
        )?;
        runtime.settings = settings;
        Ok(Json(json!({"enabled":body.enabled})))
    })
    .await
}
pub async fn configure_categories(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Categories>,
) -> Api<Json<Value>> {
    if body.password.len() > 128 || body.categories.is_empty() || body.categories.len() > 16 {
        return Err(Error::bad("最多16个分区，密码最多128字节"));
    }
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for c in &body.categories {
        if c.id != "general" && !uuid(&c.id)
            || c.id == "general" && !c.enabled
            || c.name.trim() != c.name
            || c.name.is_empty()
            || c.name.len() > 64
            || c.name.chars().any(char::is_control)
            || !ids.insert(c.id.clone())
            || !names.insert(c.name.to_lowercase())
        {
            return Err(Error::bad(
                "分区名称需唯一，长度1至64字节；综合分区必须保留启用",
            ));
        }
    }
    if !ids.contains("general") {
        return Err(Error::bad("必须保留综合分区"));
    }
    app.expensive(move |a| {
        crate::settings::admin(a, &headers)?;
        let s = a.reauthenticate(&headers, &Zeroizing::new(body.password))?;
        scope(&s, body.account)?;
        let db = a.db.connection.lock().unwrap();
        if !administrator(&db, &s)? { return Err(Error(StatusCode::FORBIDDEN,"需要管理员权限")); }
        let tx = db.unchecked_transaction().map_err(anyhow::Error::from)?;
        let revision:i64 = tx.query_row("SELECT revision FROM community_category_revision WHERE id=1",[],|r|r.get(0)).map_err(anyhow::Error::from)?;
        if revision != body.revision { return Err(Error(StatusCode::CONFLICT,"分区已被修改，请刷新后重新编辑")); }
        if categories(&tx)?.iter().any(|c| !ids.contains(&c.id)) {
            return Err(Error::bad("已有分区不可删除，可关闭以保留旧帖归属"));
        }
        for (position,c) in body.categories.iter().enumerate() {
            tx.execute("INSERT INTO community_categories(id,name,enabled,position) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,enabled=excluded.enabled,position=excluded.position",params![c.id,c.name,c.enabled,position as i64]).map_err(anyhow::Error::from)?;
        }
        tx.execute("UPDATE community_category_revision SET revision=revision+1 WHERE id=1",[]).map_err(anyhow::Error::from)?;
        tx.commit().map_err(anyhow::Error::from)?;
        Ok(Json(json!({"account":s.user.id,"revision":revision+1})))
    }).await
}
pub async fn decide(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> Api<Json<Value>> {
    app.work(move|a|{let s=a.session(&headers)?;scope(&s,body.account)?;let mut connection=a.db.connection.lock().unwrap();let db=connection.transaction().map_err(anyhow::Error::from)?;if !administrator(&db,&s)?{return Err(Error(StatusCode::FORBIDDEN,"仅管理员可处理举报"));}
  let parent:Option<String>=db.query_row("SELECT parent FROM community_items WHERE id=? AND kind='report' AND ready=1 AND deleted=0",[&id],|r|r.get(0)).optional().map_err(anyhow::Error::from)?.flatten();let parent=parent.ok_or_else(missing)?;
  if body.remove{let revision:Option<i64>=db.query_row("SELECT revision FROM community_items WHERE id=?",[&parent],|r|r.get(0)).optional().map_err(anyhow::Error::from)?;if revision.is_none()||body.reviewed_revision!=revision{return Err(Error(StatusCode::CONFLICT,"请查看举报目标的最新内容，再确认删除"));}mark_deleted(&db,&parent,s.user.id)?;}db.execute("UPDATE community_items SET decision=?,deleted=1,deleted_at=?,deleted_by=? WHERE id=?",params![if body.remove{"removed"}else{"dismissed"},now(),s.user.id,id]).map_err(anyhow::Error::from)?;db.commit().map_err(anyhow::Error::from)?;Ok(Json(json!({"account":s.user.id})))
 }).await
}
async fn clean_one(app: &Arc<App>, id: String) -> Api<()> {
    clean_one_with(app, id, |config, key| async move {
        storage::community_remove(&config, &key).await
    })
    .await
}
async fn clean_one_with<F, Fut>(app: &Arc<App>, id: String, remove: F) -> Api<()>
where
    F: FnOnce(storage::Storage, String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let Ok(_permit) = app.friend_objects.clone().try_acquire_owned() else {
        return Ok(());
    };
    let Ok(lease) = app
        .friend_transfer(&format!("community:{id}"))
        .try_lock_owned()
    else {
        return Ok(());
    };
    let mid = id.clone();
    let row=app.work(move|a|{let runtime=a.runtime.read().unwrap();let db=a.db.connection.lock().unwrap();let row:Option<(String,String)>=db.query_row("SELECT object_key,store_id FROM community_items WHERE id=? AND cleaned=0 AND (deleted=1 OR (ready=0 AND created_at<?)) AND next_attempt<=?",params![mid,now()-3600,now()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(anyhow::Error::from)?;row.map(|(key,store)|Ok((key,storage::resolve(&runtime.settings,&store)?))).transpose()}).await?;
    let Some((key, config)) = row else {
        return Ok(());
    };
    let ok = remove(config, key).await.is_ok();
    app.work(move|a|{let _lease=lease;let db=a.db.connection.lock().unwrap();if ok{db.execute("UPDATE community_items SET cleaned=1,deleted=1,deleted_at=COALESCE(deleted_at,?) WHERE id=?",params![now(),id]).map_err(anyhow::Error::from)?;}else{db.execute("UPDATE community_items SET attempts=min(attempts+1,8),next_attempt=?+min(3600,30*(1<<min(attempts,7))) WHERE id=?",params![now(),id]).map_err(anyhow::Error::from)?;}Ok(())}).await
}
pub fn start_cleanup(app: Arc<App>) {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(30));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            let rows=app.work(|a|{let db=a.db.connection.lock().unwrap();db.execute("DELETE FROM community_items WHERE cleaned=1 AND deleted_at<? AND NOT EXISTS(SELECT 1 FROM community_changes e WHERE e.kind='edit' AND e.target=community_items.id AND e.cleaned=0) AND NOT EXISTS(SELECT 1 FROM community_items child WHERE child.parent=community_items.id AND child.cleaned=0)",[now()-30*86400]).map_err(anyhow::Error::from)?;let mut stmt=db.prepare("SELECT id FROM community_items WHERE cleaned=0 AND (deleted=1 OR (ready=0 AND created_at<?)) AND next_attempt<=? ORDER BY next_attempt,seq LIMIT 2").map_err(anyhow::Error::from)?;Ok(stmt.query_map(params![now()-3600,now()],|r|r.get::<_,String>(0)).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?)}).await;
            changes::cleanup(&app).await;
            if let Ok(rows) = rows {
                for id in rows {
                    let _ = clean_one(&app, id).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn body(owner: i64, n: u32, kind: &str, parent: Option<String>) -> Draft {
        Draft {
            account: owner,
            id: format!("00000000-0000-4000-8000-{n:012x}"),
            kind: kind.into(),
            parent,
            issued_at: now(),
            category: general(),
            payload: Payload {
                title: if kind == "post" {
                    "一起玩游戏".into()
                } else {
                    String::new()
                },
                text: "literal <script>not executable</script>".into(),
                image: String::new(),
            },
        }
    }
    fn enable(app: &App) {
        let mut runtime = app.runtime.write().unwrap();
        runtime.settings.community_enabled = true;
        runtime.settings.storage = storage::Storage {
            enabled: true,
            endpoint: "https://s3.example.invalid".into(),
            bucket: "original-bucket".into(),
            region: "us-east-1".into(),
            access_key: "synthetic-access".into(),
            secret_key: "synthetic-secret-only".into(),
            retention_days: 7,
        };
    }
    fn peer(app: &App, headers: &HeaderMap) -> HeaderMap {
        let mut h = headers.clone();
        let token = app.db.create_session(2, false, "fixture hash").unwrap();
        h.insert("cookie", format!("webts_dev={token}").parse().unwrap());
        h
    }
    fn action(account: i64) -> Action {
        Action {
            account,
            liked: false,
            remove: false,
            reviewed_revision: Some(0),
        }
    }
    fn page(account: i64, kind: &str, parent: Option<String>) -> Listing {
        Listing {
            author: None,
            latest: false,
            account,
            kind: kind.into(),
            parent,
            before: None,
            category: None,
        }
    }
    #[tokio::test]
    async fn community_avatars_use_profile_versions_without_friendship_or_private_fields() {
        use axum::body::to_bytes;
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        let other = peer(&app, &h);
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(8, 8)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let saved = crate::profile::save(
            State(app.clone()),
            other.clone(),
            Json(crate::profile::Profile {
                display_name: "头像测试者".into(),
                avatar: STANDARD.encode(png.into_inner()),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;
        let hash = crate::db::digest(&saved.avatar);
        let mut root = body(2, 200, "post", None);
        let write = prepare(&app, &other, &mut root).unwrap();
        let _ = finish(&app, &other, &root).unwrap();
        let result = list(State(app.clone()), h.clone(), Query(page(1, "post", None)))
            .await
            .unwrap()
            .0;
        assert_eq!(result["items"][0]["avatar_hash"], hash);
        assert_eq!(result["items"][0]["name"], "头像测试者");
        assert!(!result.to_string().contains(&saved.avatar));
        assert!(!result.to_string().contains("audit-b@example.com"));
        let response = avatar(
            State(app.clone()),
            h.clone(),
            Path(root.id.clone()),
            Query(AvatarQuery {
                account: 1,
                v: hash.clone(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(response.headers()["content-type"], "image/png");
        assert!(
            response.headers()["cache-control"]
                .to_str()
                .unwrap()
                .contains("no-store")
        );
        assert_eq!(
            to_bytes(response.into_body(), 128 * 1024)
                .await
                .unwrap()
                .as_ref(),
            STANDARD.decode(&saved.avatar).unwrap()
        );
        let mut newer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(7, 7)
            .write_to(&mut newer, image::ImageFormat::Png)
            .unwrap();
        let saved = crate::profile::save(
            State(app.clone()),
            other.clone(),
            Json(crate::profile::Profile {
                display_name: "新昵称测试".into(),
                avatar: STANDARD.encode(newer.into_inner()),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;
        let new_hash = crate::db::digest(&saved.avatar);
        assert_ne!(hash, new_hash);
        assert_eq!(
            avatar(
                State(app.clone()),
                h.clone(),
                Path(root.id.clone()),
                Query(AvatarQuery {
                    account: 1,
                    v: hash
                })
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::CONFLICT
        );
        let content = read_complete(&app, &h, &root.id, &write.sealed).unwrap().0;
        assert_eq!(content["avatar_hash"], new_hash);
        assert_eq!(content["name"], "新昵称测试");
        app.runtime
            .write()
            .unwrap()
            .settings
            .image_limits
            .avatar_upload_kib = 8;
        assert!(
            avatar(
                State(app.clone()),
                h.clone(),
                Path(root.id.clone()),
                Query(AvatarQuery {
                    account: 1,
                    v: new_hash.clone()
                })
            )
            .await
            .is_ok()
        );
        assert_eq!(
            avatar(
                State(app.clone()),
                h.clone(),
                Path(root.id.clone()),
                Query(AvatarQuery {
                    account: 2,
                    v: new_hash.clone()
                })
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::CONFLICT
        );
        let _ = crate::profile::save(
            State(app.clone()),
            other.clone(),
            Json(crate::profile::Profile::default()),
        )
        .await
        .unwrap();
        assert!(
            avatar(
                State(app.clone()),
                h.clone(),
                Path(root.id),
                Query(AvatarQuery {
                    account: 1,
                    v: new_hash
                })
            )
            .await
            .is_err()
        );
    }
    #[tokio::test]
    async fn avatar_route_rechecks_parent_blocks_bans_module_sessions_and_deleted_content() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        let other = peer(&app, &h);
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(8, 8)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let encoded = STANDARD.encode(png.into_inner());
        let hash = crate::db::digest(&encoded);
        {
            let db = app.db.connection.lock().unwrap();
            for owner in [1, 2] {
                db.execute(
                    "INSERT INTO profiles(user_id,data,avatar_hash) VALUES(?,?,?)",
                    params![
                        owner,
                        json!({"avatar":encoded,"display_name":"合成成员"}).to_string(),
                        hash
                    ],
                )
                .unwrap();
            }
        }
        let mut root = body(2, 210, "post", None);
        prepare(&app, &other, &mut root).unwrap();
        let _ = finish(&app, &other, &root).unwrap();
        let mut comment = body(1, 211, "comment", Some(root.id.clone()));
        prepare(&app, &h, &mut comment).unwrap();
        let _ = finish(&app, &h, &comment).unwrap();
        let router = crate::app::router(app.clone());
        let call = |id: String, headers: HeaderMap, query: String| {
            let router = router.clone();
            async move {
                let mut r = Request::builder()
                    .uri(format!("/api/community/{id}/avatar?{query}"))
                    .body(Body::empty())
                    .unwrap();
                *r.headers_mut() = headers;
                router.oneshot(r).await.unwrap()
            }
        };
        let q = format!("account=1&v={hash}");
        assert_eq!(
            call(root.id.clone(), HeaderMap::new(), q.clone())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(root.id.clone(), h.clone(), q.clone()).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            call(
                root.id.clone(),
                h.clone(),
                "account=1&v=../../avatar".into()
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(root.id.clone(), h.clone(), q.clone() + "&owner=2")
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        app.db.connection.lock().unwrap().execute("INSERT INTO friendships(lo,hi,requester,status,blocked_by,created_at) VALUES(1,2,1,2,2,?)",[now()]).unwrap();
        assert_eq!(
            call(root.id.clone(), h.clone(), q.clone()).await.status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(comment.id.clone(), h.clone(), q.clone())
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM friendships", [])
            .unwrap();
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=-1 WHERE id=2", [])
            .unwrap();
        assert_eq!(
            call(root.id.clone(), h.clone(), q.clone()).await.status(),
            StatusCode::NOT_FOUND
        );
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=0 WHERE id=2", [])
            .unwrap();
        app.runtime.write().unwrap().settings.community_enabled = false;
        assert_eq!(
            call(root.id.clone(), h.clone(), q.clone()).await.status(),
            StatusCode::FORBIDDEN
        );
        app.runtime.write().unwrap().settings.community_enabled = true;
        {
            let db = app.db.connection.lock().unwrap();
            mark_deleted(&db, &root.id, 2).unwrap();
        }
        assert_eq!(
            call(comment.id, h.clone(), q.clone()).await.status(),
            StatusCode::NOT_FOUND
        );
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions", [])
            .unwrap();
        assert_eq!(call(root.id, h, q).await.status(), StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn categories_preserve_legacy_retries_filtering_and_closed_uploads() {
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        let category = "00000000-0000-4000-8000-000000000099";
        app.db
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO community_categories VALUES(?,'CS2',1,1)",
                [category],
            )
            .unwrap();
        let mut legacy = body(1, 90, "post", None);
        let first = prepare(&app, &h, &mut legacy).unwrap();
        let legacy_binding = serde_json::to_vec(&(
            &legacy.kind,
            &legacy.parent,
            legacy.issued_at,
            &legacy.payload,
        ))
        .unwrap();
        assert_eq!(
            app.db
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT request_hash FROM community_items WHERE id=?",
                    [&legacy.id],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            app.vault
                .request_tag("community-request-v1", &legacy_binding)
        );
        let _ = finish(&app, &h, &legacy).unwrap();
        let mut draft = body(1, 91, "post", None);
        draft.category = category.into();
        let write = prepare(&app, &h, &mut draft).unwrap();
        app.db
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE community_categories SET enabled=0 WHERE id=?",
                [category],
            )
            .unwrap();
        assert_eq!(prepare(&app, &h, &mut draft).unwrap().key, write.key);
        let _ = finish(&app, &h, &draft).unwrap();
        let mut new = body(1, 92, "post", None);
        new.category = category.into();
        assert!(prepare(&app, &h, &mut new).is_err());
        draft.category = general();
        assert_eq!(
            prepare(&app, &h, &mut draft).err().unwrap().0,
            StatusCode::CONFLICT
        );
        let mut q = page(1, "post", None);
        q.category = Some(category.into());
        let result = list(State(app.clone()), h.clone(), Query(q))
            .await
            .unwrap()
            .0;
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
        assert_eq!(result["items"][0]["category"], category);
        assert_eq!(result["categories"][1]["enabled"], false);
        let mut q = page(1, "post", None);
        q.category = Some(general());
        assert_eq!(
            list(State(app.clone()), h.clone(), Query(q))
                .await
                .unwrap()
                .0["items"][0]["id"],
            legacy.id
        );
        assert_eq!(
            read_complete(&app, &h, &legacy.id, &first.sealed)
                .unwrap()
                .0["content"]["text"],
            legacy.payload.text
        );
        let mut q = page(1, "comment", Some(legacy.id));
        q.category = Some(category.into());
        assert_eq!(
            list(State(app.clone()), h.clone(), Query(q))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::BAD_REQUEST
        );
        let mut forged = body(1, 93, "post", None);
        forged.category = "00000000-0000-4000-8000-000000000098".into();
        assert!(prepare(&app, &h, &mut forged).is_err());
        let other = peer(&app, &h);
        let mut q = page(1, "post", None);
        q.category = Some(general());
        assert_eq!(
            list(State(app), other, Query(q)).await.err().unwrap().0,
            StatusCode::CONFLICT
        );
    }
    #[tokio::test]
    async fn category_configuration_requires_fresh_admin_password_and_revision() {
        let (_dir, app, h) = crate::app::tests::fixture();
        let pw = "synthetic-category-passphrase";
        let hash = crate::password::hash(pw).unwrap();
        app.db
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE users SET password_hash=?,is_admin=1 WHERE id=1",
                [hash],
            )
            .unwrap();
        let value = json!({"account":1,"password":pw,"revision":0,"categories":[{"id":"general","name":"综合","enabled":true},{"id":"00000000-0000-4000-8000-000000000099","name":"CS2","enabled":true}]});
        let run = |v: Value| {
            configure_categories(
                State(app.clone()),
                h.clone(),
                Json(serde_json::from_value(v).unwrap()),
            )
        };
        let mut bad = value.clone();
        bad["password"] = json!("incorrect");
        assert!(run(bad).await.is_err());
        let mut bad = value.clone();
        bad["account"] = json!(2);
        assert!(run(bad).await.is_err());
        assert_eq!(run(value.clone()).await.unwrap().0["revision"], 1);
        assert_eq!(
            run(value.clone()).await.err().unwrap().0,
            StatusCode::CONFLICT
        );
        let mut missing = value.clone();
        missing["revision"] = json!(1);
        missing["categories"].as_array_mut().unwrap().pop();
        assert!(run(missing).await.is_err());
        for name in ["", " CS2", "综合", "bad\nname"] {
            let mut bad = value.clone();
            bad["revision"] = json!(1);
            bad["categories"][1]["name"] = json!(name);
            assert!(run(bad).await.is_err());
        }
        let mut disabled = value.clone();
        disabled["revision"] = json!(1);
        disabled["categories"][0]["enabled"] = json!(false);
        assert!(run(disabled).await.is_err());
        let mut updated = value;
        updated["revision"] = json!(1);
        updated["categories"][1]["name"] = json!("反恐精英");
        updated["categories"][1]["enabled"] = json!(false);
        updated["categories"].as_array_mut().unwrap().reverse();
        assert_eq!(run(updated.clone()).await.unwrap().0["revision"], 2);
        {
            let db = app.db.connection.lock().unwrap();
            let c = categories(&db).unwrap();
            assert_eq!(c[0].name, "反恐精英");
            assert!(!c[0].enabled);
        }
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=0 WHERE id=1", [])
            .unwrap();
        updated["revision"] = json!(2);
        assert!(run(updated).await.is_err());
    }
    #[tokio::test]
    async fn category_migration_preserves_old_metadata_on_reopen() {
        let (dir, _, _) = crate::app::tests::fixture();
        let path = dir.path().join("legacy.sqlite");
        {
            let db = crate::db::Db::open(path.to_str().unwrap()).unwrap();
            let c = db.connection.lock().unwrap();
            c.execute_batch("INSERT INTO users(id,email,password_hash,verified) VALUES(1,'legacy@example.invalid','hash',1); INSERT INTO community_items(id,owner,kind,object_key,store_id,request_hash,created_at,ready) VALUES('old',1,'post','untouched-key','default','original-binding',1,1); DROP INDEX community_category_listing; ALTER TABLE community_items DROP COLUMN category; DROP TABLE community_categories; DROP TABLE community_category_revision;").unwrap();
        }
        for _ in 0..2 {
            let db = crate::db::Db::open(path.to_str().unwrap()).unwrap();
            let c = db.connection.lock().unwrap();
            assert_eq!(c.query_row("SELECT category||':'||request_hash||':'||object_key FROM community_items WHERE id='old'",[],|r|r.get::<_,String>(0)).unwrap(),"general:original-binding:untouched-key");
            assert_eq!(categories(&c).unwrap().len(), 1);
        }
    }
    #[tokio::test]
    async fn encrypted_objects_scope_idempotency_and_original_store_are_preserved() {
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        let other = peer(&app, &h);
        let mut draft = body(1, 1, "post", None);
        let first = prepare(&app, &h, &mut draft).unwrap();
        assert!(!first.ready);
        assert_eq!(
            list(State(app.clone()), h.clone(), Query(page(1, "post", None)))
                .await
                .unwrap()
                .0["items"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert!(!first.sealed.windows(7).any(|v| v == b"literal"));
        {
            let mut runtime = app.runtime.write().unwrap();
            let mut config = runtime.settings.storage.clone();
            config.bucket = "next-bucket".into();
            runtime.settings.storage_profiles.push(storage::Profile {
                id: "a".repeat(64),
                name: "Other".into(),
                storage: config,
            });
            runtime.settings.storage_active = "a".repeat(64);
        }
        let again = prepare(&app, &h, &mut draft).unwrap();
        assert_eq!(again.key, first.key);
        assert_eq!(again.config.bucket, "original-bucket");
        let _ = finish(&app, &h, &draft).unwrap();
        assert_eq!(
            read_complete(&app, &other, &draft.id, &first.sealed)
                .unwrap()
                .0["content"]["text"],
            draft.payload.text
        );
        let mut vote = action(2);
        vote.liked = true;
        let _ = like(
            State(app.clone()),
            other.clone(),
            Path(draft.id.clone()),
            Json(vote),
        )
        .await
        .unwrap();
        let counts = read_complete(&app, &other, &draft.id, &first.sealed)
            .unwrap()
            .0;
        assert_eq!(counts["liked"], true);
        assert_eq!(counts["likes"], 1);
        let mut switched = body(2, 1, "post", None);
        assert_eq!(
            prepare(&app, &other, &mut switched).err().unwrap().0,
            StatusCode::CONFLICT
        );
        draft.payload.text = "different".into();
        assert_eq!(
            prepare(&app, &h, &mut draft).err().unwrap().0,
            StatusCode::CONFLICT
        );
        let mut wrong = body(2, 2, "post", None);
        assert_eq!(
            prepare(&app, &h, &mut wrong).err().unwrap().0,
            StatusCode::CONFLICT
        );
        let mut next = body(1, 3, "post", None);
        assert_eq!(
            prepare(&app, &h, &mut next).unwrap().config.bucket,
            "next-bucket"
        );
    }
    #[tokio::test]
    async fn deletion_blocks_late_read_and_comment_commit_without_granting_other_users_delete() {
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        let other = peer(&app, &h);
        let mut root = body(1, 1, "post", None);
        let stored = prepare(&app, &h, &mut root).unwrap();
        let _ = finish(&app, &h, &root).unwrap();
        let mut comment = body(2, 2, "comment", Some(root.id.clone()));
        prepare(&app, &other, &mut comment).unwrap();
        assert_eq!(
            delete(
                State(app.clone()),
                other.clone(),
                Path(root.id.clone()),
                Json(action(2))
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::NOT_FOUND
        );
        let _ = delete(
            State(app.clone()),
            h.clone(),
            Path(root.id.clone()),
            Json(action(1)),
        )
        .await
        .unwrap();
        assert_eq!(
            read_complete(&app, &other, &root.id, &stored.sealed)
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
        assert!(finish(&app, &other, &comment).is_err());
        assert!(prepare(&app, &h, &mut root).is_err());
        let marked: i64 = app
            .db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM community_items WHERE deleted=1 AND deleted_by=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(marked, 2);
    }
    #[tokio::test]
    async fn block_ban_session_and_module_revocation_are_rechecked_before_plaintext() {
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        let other = peer(&app, &h);
        let mut root = body(1, 1, "post", None);
        let stored = prepare(&app, &h, &mut root).unwrap();
        let _ = finish(&app, &h, &root).unwrap();
        app.db.connection.lock().unwrap().execute("INSERT INTO friendships(lo,hi,requester,status,blocked_by,created_at) VALUES(1,2,1,2,2,0)",[]).unwrap();
        assert!(read_complete(&app, &other, &root.id, &stored.sealed).is_err());
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM friendships", [])
            .unwrap();
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=-1 WHERE id=1", [])
            .unwrap();
        assert!(read_complete(&app, &other, &root.id, &stored.sealed).is_err());
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=0 WHERE id=1", [])
            .unwrap();
        app.runtime.write().unwrap().settings.community_enabled = false;
        assert_eq!(
            read_complete(&app, &other, &root.id, &stored.sealed)
                .err()
                .unwrap()
                .0,
            StatusCode::FORBIDDEN
        );
        app.runtime.write().unwrap().settings.community_enabled = true;
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE user_id=2", [])
            .unwrap();
        assert_eq!(
            read_complete(&app, &other, &root.id, &stored.sealed)
                .err()
                .unwrap()
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn cleanup_uses_original_bucket_skips_active_upload_and_retries_failures() {
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        let mut root = body(1, 1, "post", None);
        prepare(&app, &h, &mut root).unwrap();
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE community_items SET created_at=?", [now() - 3601])
            .unwrap();
        let lease = app
            .friend_transfer(&format!("community:{}", root.id))
            .lock_owned()
            .await;
        clean_one_with(&app, root.id.clone(), |_, _| async {
            panic!("must not remove active PUT")
        })
        .await
        .unwrap();
        drop(lease);
        clean_one_with(&app, root.id.clone(), |config, _| async move {
            assert_eq!(config.bucket, "original-bucket");
            Err(anyhow::anyhow!("synthetic failure"))
        })
        .await
        .unwrap();
        let (cleaned, attempts, next): (bool, i64, i64) = app
            .db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT cleaned,attempts,next_attempt FROM community_items",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert!(!cleaned);
        assert_eq!(attempts, 1);
        assert!(next > now());
        clean_one_with(&app, root.id.clone(), |_, _| async {
            panic!("backoff must not be bypassed")
        })
        .await
        .unwrap();
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE community_items SET next_attempt=0", [])
            .unwrap();
        clean_one_with(&app, root.id.clone(), |_, _| async { Ok(()) })
            .await
            .unwrap();
        assert!(
            app.db
                .connection
                .lock()
                .unwrap()
                .query_row("SELECT cleaned AND deleted FROM community_items", [], |r| r
                    .get::<_, bool>(0))
                .unwrap()
        );
    }
    #[tokio::test]
    async fn reports_are_private_and_moderation_stays_available_while_member_access_is_off() {
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        let other = peer(&app, &h);
        let mut root = body(1, 1, "post", None);
        prepare(&app, &h, &mut root).unwrap();
        let _ = finish(&app, &h, &root).unwrap();
        let mut report = body(2, 2, "report", Some(root.id.clone()));
        let saved = prepare(&app, &other, &mut report).unwrap();
        let _ = finish(&app, &other, &report).unwrap();
        assert!(read_complete(&app, &h, &report.id, &saved.sealed).is_err());
        assert!(
            list(
                State(app.clone()),
                other.clone(),
                Query(page(2, "report", None))
            )
            .await
            .is_err()
        );
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=1 WHERE id=1", [])
            .unwrap();
        app.runtime.write().unwrap().settings.community_enabled = false;
        assert_eq!(
            list(
                State(app.clone()),
                h.clone(),
                Query(page(1, "report", None))
            )
            .await
            .unwrap()
            .0["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(read_complete(&app, &h, &report.id, &saved.sealed).is_ok());
        let mut decision = action(1);
        decision.remove = true;
        let _ = decide(State(app.clone()), h, Path(report.id), Json(decision))
            .await
            .unwrap();
        let decision: String = app
            .db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT decision FROM community_items WHERE kind='report'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(decision, "removed");
    }
    #[tokio::test]
    async fn bounded_pagination_likes_and_invalid_media_do_not_bypass_limits() {
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        for n in 1..=14 {
            let mut root = body(1, n, "post", None);
            prepare(&app, &h, &mut root).unwrap();
            let _ = finish(&app, &h, &root).unwrap();
        }
        let first = list(State(app.clone()), h.clone(), Query(page(1, "post", None)))
            .await
            .unwrap()
            .0;
        assert_eq!(first["items"].as_array().unwrap().len(), 12);
        assert!(first["next"].as_i64().is_some());
        let id = first["items"][0]["id"].as_str().unwrap().to_owned();
        for _ in 0..2 {
            let mut a = action(1);
            a.liked = true;
            let _ = like(State(app.clone()), h.clone(), Path(id.clone()), Json(a))
                .await
                .unwrap();
        }
        assert_eq!(
            app.db
                .connection
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM community_likes", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let mut bad = body(1, 99, "post", None);
        bad.payload.image = "data:image/svg+xml,<svg/>".into();
        assert!(prepare(&app, &h, &mut bad).is_err());
        bad.payload.image.clear();
        bad.issued_at = now() - 3601;
        assert!(prepare(&app, &h, &mut bad).is_err());
        bad.issued_at = now();
        bad.payload.text = "x".repeat(4001);
        assert!(prepare(&app, &h, &mut bad).is_err());
        assert!(serde_json::from_value::<Draft>(json!({"account":1,"id":id,"kind":"post","parent":null,"issued_at":now(),"payload":{"title":"ok","text":"ok","image":"","script":"not allowed"}})).is_err());
    }
    #[tokio::test]
    async fn http_routes_enforce_origin_session_scope_schema_body_limit_and_retry() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        let mut draft = body(1, 150, "post", None);
        prepare(&app, &h, &mut draft).unwrap();
        let _ = finish(&app, &h, &draft).unwrap();
        let valid = json!({"account":1,"id":draft.id,"kind":"post","parent":null,"issued_at":draft.issued_at,"payload":draft.payload}).to_string();
        let router = crate::app::router(app.clone());
        for (path, method, body, headers, expected) in [
            (
                "/api/community?account=1",
                "GET",
                String::new(),
                HeaderMap::new(),
                StatusCode::UNAUTHORIZED,
            ),
            (
                "/api/community?account=2",
                "GET",
                String::new(),
                h.clone(),
                StatusCode::CONFLICT,
            ),
            (
                "/api/community?account=1&kind=report",
                "GET",
                String::new(),
                h.clone(),
                StatusCode::FORBIDDEN,
            ),
            (
                "/api/admin/community",
                "GET",
                String::new(),
                h.clone(),
                StatusCode::FORBIDDEN,
            ),
            (
                "/api/community",
                "POST",
                valid.clone(),
                {
                    let mut v = h.clone();
                    v.insert("origin", "https://evil.invalid".parse().unwrap());
                    v
                },
                StatusCode::FORBIDDEN,
            ),
            (
                "/api/community",
                "POST",
                "{}".into(),
                h.clone(),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                "/api/community",
                "POST",
                valid.replace("\"payload\":{", "\"payload\":{\"script\":true,"),
                h.clone(),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                "/api/community",
                "POST",
                " ".repeat(256 * 1024 + 1),
                h.clone(),
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            ("/api/admin/community/categories", "POST", json!({"account":1,"revision":0,"password":"synthetic","categories":[{"id":"general","name":"综合","enabled":true}]}).to_string(), h.clone(), StatusCode::FORBIDDEN),
            ("/api/admin/community/categories", "POST", " ".repeat(16*1024+1), h.clone(), StatusCode::PAYLOAD_TOO_LARGE),
            ("/api/admin/community/categories", "POST", "{}".into(), {let mut v=h.clone();v.insert("origin","https://evil.invalid".parse().unwrap());v}, StatusCode::FORBIDDEN),
            // An already-committed exact retry succeeds without network access.
            ("/api/community", "POST", valid, h.clone(), StatusCode::OK),
        ] {
            let mut request = Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap();
            *request.headers_mut() = headers;
            request
                .headers_mut()
                .insert("content-type", "application/json".parse().unwrap());
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), expected, "{method} {path}");
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
    }
    #[tokio::test]
    async fn storage_statistics_keep_deleted_community_objects_until_remote_cleanup() {
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=1 WHERE id=1", [])
            .unwrap();
        let mut draft = body(1, 155, "post", None);
        prepare(&app, &h, &mut draft).unwrap();
        let view = storage::profiles(State(app.clone()), h.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(view["profiles"][0]["objects"], 1);
        assert_eq!(view["profiles"][0]["storage"]["objects"]["community"], 1);
        let _ = delete(
            State(app.clone()),
            h.clone(),
            Path(draft.id.clone()),
            Json(action(1)),
        )
        .await
        .unwrap();
        assert_eq!(
            storage::profiles(State(app.clone()), h.clone())
                .await
                .unwrap()
                .0["profiles"][0]["objects"],
            1
        );
        clean_one_with(&app, draft.id, |_, _| async { Ok(()) })
            .await
            .unwrap();
        assert_eq!(
            storage::profiles(State(app), h).await.unwrap().0["profiles"][0]["objects"],
            0
        );
    }
    #[tokio::test]
    async fn administrator_overview_is_scoped_counts_cleanup_and_refuses_revoked_role() {
        let (_dir, app, h) = crate::app::tests::fixture();
        enable(&app);
        assert_eq!(
            admin_config(State(app.clone()), h.clone())
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::FORBIDDEN
        );
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=1 WHERE id=1", [])
            .unwrap();
        let empty = admin_config(State(app.clone()), h.clone()).await.unwrap().0;
        assert_eq!(empty["account"], 1);
        assert_eq!(empty["stats"]["total"], 0);
        assert_eq!(empty["stats"]["storage_configured"], true);
        let mut draft = body(1, 170, "post", None);
        prepare(&app, &h, &mut draft).unwrap();
        assert_eq!(
            admin_config(State(app.clone()), h.clone()).await.unwrap().0["stats"]["uploads"],
            1
        );
        let _ = finish(&app, &h, &draft).unwrap();
        assert_eq!(
            admin_config(State(app.clone()), h.clone()).await.unwrap().0["stats"]["posts"],
            1
        );
        let _ = delete(
            State(app.clone()),
            h.clone(),
            Path(draft.id.clone()),
            Json(action(1)),
        )
        .await
        .unwrap();
        clean_one_with(&app, draft.id.clone(), |_, _| async {
            anyhow::bail!("synthetic provider failure")
        })
        .await
        .unwrap();
        let failed = admin_config(State(app.clone()), h.clone()).await.unwrap().0;
        assert_eq!(failed["stats"]["posts"], 0);
        assert_eq!(failed["stats"]["cleanup"], 1);
        assert_eq!(failed["stats"]["retries"], 1);
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE community_items SET next_attempt=0", [])
            .unwrap();
        clean_one_with(&app, draft.id, |_, _| async { Ok(()) })
            .await
            .unwrap();
        let cleaned = admin_config(State(app.clone()), h.clone()).await.unwrap().0;
        assert_eq!(cleaned["stats"]["cleanup"], 0);
        assert_eq!(cleaned["stats"]["retries"], 0);
        assert_eq!(cleaned["stats"]["total"], 1);
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=0 WHERE id=1", [])
            .unwrap();
        assert_eq!(
            admin_config(State(app), h).await.err().unwrap().0,
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn author_history_requires_a_visible_anchor_and_reveals_only_readable_posts() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let other = peer(&a, &h);
        let mut own = body(1, 401, "post", None);
        let own_write = prepare(&a, &h, &mut own).unwrap();
        let _ = finish(&a, &h, &own).unwrap();
        let mut theirs = body(2, 402, "post", None);
        prepare(&a, &other, &mut theirs).unwrap();
        let _ = finish(&a, &other, &theirs).unwrap();
        let mut reply = body(2, 403, "comment", Some(own.id.clone()));
        prepare(&a, &other, &mut reply).unwrap();
        let _ = finish(&a, &other, &reply).unwrap();
        let mut q = page(1, "post", None);
        q.author = Some(reply.id.clone());
        let r = list(State(a.clone()), h.clone(), Query(q)).await.unwrap().0;
        assert_eq!(r["author"]["owner"], 2);
        assert_eq!(r["items"].as_array().unwrap().len(), 1);
        assert_eq!(r["items"][0]["id"], theirs.id);
        assert!(!r.to_string().contains("email"));
        assert!(!r.to_string().contains("audit-b@example.com"));
        let mut q = page(1, "post", None);
        q.author = Some("2".into());
        assert_eq!(
            list(State(a.clone()), h.clone(), Query(q))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::BAD_REQUEST
        );
        let mut q = page(1, "post", None);
        q.author = Some("00000000-0000-4000-8000-000000009999".into());
        assert_eq!(
            list(State(a.clone()), h.clone(), Query(q))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            read_complete(&a, &h, &own.id, &own_write.sealed).unwrap().0["comments"],
            1
        );
        {
            let db = a.db.connection.lock().unwrap();
            db.execute("INSERT INTO friendships(lo,hi,requester,status,blocked_by,created_at) VALUES(1,2,1,2,2,?)",[now()]).unwrap();
        }
        let session = a.session(&h).unwrap();
        assert_eq!(
            comment_count(&a.db.connection.lock().unwrap(), &session, &own.id).unwrap(),
            0
        );
        let mut q = page(1, "post", None);
        q.author = Some(theirs.id);
        assert_eq!(
            list(State(a.clone()), h.clone(), Query(q))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
    }
    #[tokio::test]
    async fn latest_digest_preserves_time_order_instead_of_replacing_it_with_old_pins() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let mut old = body(1, 411, "post", None);
        prepare(&a, &h, &mut old).unwrap();
        let _ = finish(&a, &h, &old).unwrap();
        let mut recent = body(1, 412, "post", None);
        prepare(&a, &h, &mut recent).unwrap();
        let _ = finish(&a, &h, &recent).unwrap();
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE community_items SET pinned=1 WHERE id=?", [&old.id])
            .unwrap();
        let regular = list(State(a.clone()), h.clone(), Query(page(1, "post", None)))
            .await
            .unwrap()
            .0;
        assert_eq!(regular["items"][0]["id"], old.id);
        let mut q = page(1, "post", None);
        q.latest = true;
        let digest = list(State(a), h, Query(q)).await.unwrap().0;
        assert_eq!(digest["items"][0]["id"], recent.id);
        assert_eq!(digest["items"][1]["id"], old.id);
    }
}
