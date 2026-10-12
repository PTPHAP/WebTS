//! Login-only events. Invitations locate an event; they never grant access or TS permissions.
use crate::{
    app::{Api, App, Error},
    db::now,
    preferences::session_valid,
    vault::token,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    title: String,
    #[serde(default)]
    description: String,
    starts: i64,
    ends: i64,
    server: String,
    channel: u64,
    #[serde(default)]
    community: bool,
    #[serde(default)]
    invitees: Vec<i64>,
    request_id: String,
}
impl Draft {
    fn validate(&self) -> Api<()> {
        if self.title.trim().is_empty()
            || self.title.len() > 240
            || self.title.chars().any(char::is_control)
            || self.description.len() > 4000
            || self
                .description
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
            || !self
                .ends
                .checked_sub(self.starts)
                .is_some_and(|v| (1..=8 * 3600).contains(&v))
            || self.channel > 9_007_199_254_740_991
            || self.server.len() > 64
            || self.invitees.len() > 64
            || self.invitees.iter().any(|v| *v <= 0)
            || self.invitees.iter().copied().collect::<HashSet<_>>().len() != self.invitees.len()
            || self.request_id.len() != 36
            || !self
                .request_id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b'-')
        {
            return Err(Error::bad(
                "活动内容、时间或邀请名单无效；开始需在1分钟至90天内，时长最多8小时",
            ));
        }
        if self.community && !self.invitees.is_empty() {
            return Err(Error::bad("全站活动无需邀请名单"));
        }
        Ok(())
    }
    fn validate_time(&self) -> Api<()> {
        if self.starts < now() + 60 || self.starts > now() + 90 * 86400 {
            return Err(Error::bad("活动开始需在1分钟至90天内"));
        }
        Ok(())
    }
}
pub(crate) fn cleanup(db: &rusqlite::Connection) -> Api<()> {
    db.execute(
        "DELETE FROM community_events WHERE ends<?",
        [now() - 30 * 86400],
    )
    .map_err(anyhow::Error::from)?;
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    action: String,
}
fn identifier(id: &str) -> Api<()> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error(StatusCode::NOT_FOUND, "活动不存在或不可见"));
    }
    Ok(())
}
fn missing() -> Error {
    Error(StatusCode::NOT_FOUND, "活动不存在或不可见")
}
fn accessible(db: &rusqlite::Connection, owner: i64, id: &str) -> Api<bool> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM community_events e WHERE e.id=? AND (e.owner=? OR e.community=1 OR EXISTS(SELECT 1 FROM community_invites i JOIN friendships f ON f.lo=min(e.owner,i.user_id) AND f.hi=max(e.owner,i.user_id) AND f.status=1 AND f.blocked_by=0 WHERE i.event_id=e.id AND i.user_id=?)))",params![id,owner,owner],|r|r.get(0)).map_err(anyhow::Error::from).map_err(Into::into)
}
pub async fn list(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a|{
        let s=a.session(&headers)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let mut q=db.prepare("SELECT e.id,e.owner,e.ciphertext,e.starts,e.ends,e.community,e.cancelled,COALESCE((SELECT attending FROM community_invites WHERE event_id=e.id AND user_id=?),0),(SELECT COUNT(*) FROM community_invites i WHERE i.event_id=e.id AND i.attending=1 AND (e.community=1 OR i.user_id=e.owner OR EXISTS(SELECT 1 FROM friendships f WHERE f.lo=min(e.owner,i.user_id) AND f.hi=max(e.owner,i.user_id) AND f.status=1 AND f.blocked_by=0))) FROM community_events e WHERE e.ends>? AND (e.owner=? OR e.community=1 OR EXISTS(SELECT 1 FROM community_invites i JOIN friendships f ON f.lo=min(e.owner,i.user_id) AND f.hi=max(e.owner,i.user_id) AND f.status=1 AND f.blocked_by=0 WHERE i.event_id=e.id AND i.user_id=?)) ORDER BY e.starts,e.id LIMIT 100").map_err(anyhow::Error::from)?;
        let rows=q.query_map(params![s.user.id,now()-86400,s.user.id,s.user.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,bool>(5)?,r.get::<_,bool>(6)?,r.get::<_,bool>(7)?,r.get::<_,i64>(8)?))).map_err(anyhow::Error::from)?;
        let mut items=Vec::new();
        for row in rows{let(id,owner,record,starts,ends,community,cancelled,attending,count)=row.map_err(anyhow::Error::from)?;let bytes=a.vault.open(owner,&id,"community-event-v1",&record)?;let draft:Draft=serde_json::from_slice(&bytes).map_err(anyhow::Error::from)?;
            items.push(json!({"id":id,"mine":owner==s.user.id,"title":draft.title,"description":draft.description,"starts":starts,"ends":ends,"server":draft.server,"channel":draft.channel,"community":community,"cancelled":cancelled,"attending":attending,"participants":count}));
        }
        Ok(Json(json!({"items":items,"server_time":now()})))
    }).await
}
pub async fn create(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Draft>,
) -> Api<Json<Value>> {
    body.validate()?;
    app.work(move |a|{
        let s=a.session(&headers)?;
        if body.community&&!s.user.is_admin{return Err(Error(StatusCode::FORBIDDEN,"仅管理员可发布全站活动"));}
        // Match the project-wide order: copy runtime configuration before taking the DB mutex.
        if !a.runtime.read().unwrap().settings.servers.iter().any(|v|v.id==body.server){return Err(Error::bad("请选择站点配置的服务器"));}
        let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(anyhow::Error::from)?;session_valid(&tx,&s)?;
        let current_admin:bool=tx.query_row("SELECT is_admin FROM users WHERE id=?",[s.user.id],|r|r.get(0)).map_err(anyhow::Error::from)?;
        if body.community&&!current_admin{return Err(Error(StatusCode::FORBIDDEN,"管理员权限已撤销"));}
        let hash=crate::db::digest(&serde_json::to_string(&body).map_err(anyhow::Error::from)?);
        let existing:Option<(String,String)>=tx.query_row("SELECT id,request_hash FROM community_events WHERE owner=? AND request_id=?",params![s.user.id,body.request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(anyhow::Error::from)?;
        if let Some((id,old))=existing{if hash!=old{return Err(Error(StatusCode::CONFLICT,"重复请求内容发生变化"));}return Ok(Json(json!({"id":id})));}
        body.validate_time()?;
        cleanup(&tx)?;
        let (total,owned):(i64,i64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM(owner=? AND ends>?),0) FROM community_events",params![s.user.id,now()],|r|Ok((r.get(0)?,r.get(1)?))).map_err(anyhow::Error::from)?;
        if total>=1000||owned>=16{return Err(Error(StatusCode::TOO_MANY_REQUESTS,"活动数量已达上限，请等待旧活动过期"));}
        for peer in &body.invitees{if *peer==s.user.id{return Err(Error::bad("无需邀请自己"));}let allowed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM friendships f JOIN users u ON u.id=? WHERE f.lo=min(?,?) AND f.hi=max(?,?) AND f.status=1 AND f.blocked_by=0 AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?)",params![peer,s.user.id,peer,s.user.id,peer,now()],|r|r.get(0)).map_err(anyhow::Error::from)?;if !allowed{return Err(Error::bad("只能邀请已接受且未屏蔽的好友"));}}
        let id=token()?;let ciphertext=a.vault.seal(s.user.id,&id,"community-event-v1",serde_json::to_string(&body).map_err(anyhow::Error::from)?.as_bytes())?;
        tx.execute("INSERT INTO community_events(id,owner,request_id,request_hash,ciphertext,starts,ends,community) VALUES(?,?,?,?,?,?,?,?)",params![id,s.user.id,body.request_id,hash,ciphertext,body.starts,body.ends,body.community]).map_err(anyhow::Error::from)?;
        for peer in std::iter::once(&s.user.id).chain(body.invitees.iter()){tx.execute("INSERT INTO community_invites(event_id,user_id,attending) VALUES(?,?,?)",params![id,peer,*peer==s.user.id]).map_err(anyhow::Error::from)?;}
        tx.commit().map_err(anyhow::Error::from)?;Ok(Json(json!({"id":id})))
    }).await
}
pub async fn action(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> Api<Json<Value>> {
    identifier(&id)?;
    if !matches!(body.action.as_str(), "attend" | "leave" | "cancel") {
        return Err(Error::bad("活动操作无效"));
    }
    app.work(move |a|{let s=a.session(&headers)?;let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(anyhow::Error::from)?;session_valid(&tx,&s)?;if !accessible(&tx,s.user.id,&id)?{return Err(missing());}
        let(owner,ends,cancelled):(i64,i64,bool)=tx.query_row("SELECT owner,ends,cancelled FROM community_events WHERE id=?",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(anyhow::Error::from)?;
        if body.action=="cancel"{let current_admin:bool=tx.query_row("SELECT is_admin FROM users WHERE id=?",[s.user.id],|r|r.get(0)).map_err(anyhow::Error::from)?;if owner!=s.user.id&&!current_admin{return Err(Error(StatusCode::FORBIDDEN,"只有发起人或管理员可以取消活动"));}tx.execute("UPDATE community_events SET cancelled=1 WHERE id=?",[&id]).map_err(anyhow::Error::from)?;}
        else {if ends<=now()||cancelled{return Err(Error(StatusCode::CONFLICT,"活动已结束或取消"));}let count:i64=tx.query_row("SELECT COUNT(*) FROM community_invites WHERE event_id=?",[&id],|r|r.get(0)).map_err(anyhow::Error::from)?;let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM community_invites WHERE event_id=? AND user_id=?)",params![id,s.user.id],|r|r.get(0)).map_err(anyhow::Error::from)?;if !exists&&count>=1000{return Err(Error(StatusCode::CONFLICT,"活动报名人数已达上限"));}tx.execute("INSERT INTO community_invites(event_id,user_id,attending) VALUES(?,?,?) ON CONFLICT(event_id,user_id) DO UPDATE SET attending=excluded.attending",params![id,s.user.id,body.action=="attend"]).map_err(anyhow::Error::from)?;}
        tx.commit().map_err(anyhow::Error::from)?;Ok(Json(json!({"ok":true})))
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    fn setup() -> (tempfile::TempDir, Arc<App>, HeaderMap, HeaderMap, HeaderMap) {
        let (dir, app, first) = crate::app::tests::fixture();
        app.runtime.write().unwrap().settings.servers = vec![crate::config::Server {
            id: "fixture".into(),
            name: "Synthetic server".into(),
            address: "1.1.1.1:9987".into(),
        }];
        let (_, _, verify) = app
            .db
            .register("third@example.invalid", "fixture hash")
            .unwrap();
        app.db.consume_email_token(&verify, "verify", None).unwrap();
        let session = |id| {
            let token = app.db.create_session(id, false, "fixture hash").unwrap();
            let mut h = first.clone();
            h.insert("cookie", format!("webts_dev={token}").parse().unwrap());
            h
        };
        let second = session(2);
        let third = session(3);
        app.db.connection.lock().unwrap().execute("INSERT INTO friendships(lo,hi,requester,status,blocked_by,created_at) VALUES(1,2,1,1,0,?)",[now()]).unwrap();
        (dir, app, first, second, third)
    }
    fn draft() -> Value {
        json!({"title":"Synthetic <script>fixture</script>","description":"No real user data","starts":now()+3600,"ends":now()+7200,"server":"fixture","channel":7,"community":false,"invitees":[2],"request_id":"12345678-1234-1234-1234-123456789abc"})
    }
    async fn call(
        app: &Arc<App>,
        headers: &HeaderMap,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(if body.is_some() { "POST" } else { "GET" })
            .uri(format!("/api{path}"))
            .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
            .unwrap();
        *req.headers_mut() = headers.clone();
        req.headers_mut()
            .insert("content-type", "application/json".parse().unwrap());
        let response = crate::app::router(app.clone()).oneshot(req).await.unwrap();
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    #[tokio::test]
    async fn invitation_requires_active_friendship_and_never_grants_access_to_strangers() {
        let (_dir, a, first, second, third) = setup();
        let body = draft();
        let (status, created) = call(&a, &first, "/events", Some(body.clone())).await;
        assert_eq!(status, StatusCode::OK);
        let id = created["id"].as_str().unwrap();
        assert_eq!(
            call(&a, &second, "/events", None).await.1["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            call(&a, &third, "/events", None).await.1["items"],
            json!([])
        );
        assert_eq!(
            call(
                &a,
                &third,
                &format!("/events/{id}"),
                Some(json!({"action":"attend"}))
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(
                &a,
                &second,
                &format!("/events/{id}"),
                Some(json!({"action":"cancel"}))
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &a,
                &second,
                &format!("/events/{id}"),
                Some(json!({"action":"attend"}))
            )
            .await
            .0,
            StatusCode::OK
        );
        let record: Vec<u8> =
            a.db.connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT ciphertext FROM community_events WHERE id=?",
                    [id],
                    |r| r.get(0),
                )
                .unwrap();
        assert!(!String::from_utf8_lossy(&record).contains("Synthetic"));
        for sql in [
            "UPDATE friendships SET status=2,blocked_by=2",
            "DELETE FROM friendships",
        ] {
            a.db.connection.lock().unwrap().execute(sql, []).unwrap();
            assert_eq!(
                call(&a, &second, "/events", None).await.1["items"],
                json!([])
            );
            assert_eq!(
                call(
                    &a,
                    &second,
                    &format!("/events/{id}"),
                    Some(json!({"action":"attend"}))
                )
                .await
                .0,
                StatusCode::NOT_FOUND
            );
        }
        assert_eq!(
            call(&a, &first, "/events", None).await.1["items"][0]["participants"],
            1
        );
    }
    #[tokio::test]
    async fn duplicate_request_is_idempotent_and_changed_retries_conflict() {
        let (_dir, a, first, _, _) = setup();
        let body = draft();
        let (_, one) = call(&a, &first, "/events", Some(body.clone())).await;
        let (_, two) = call(&a, &first, "/events", Some(body.clone())).await;
        assert_eq!(one, two);
        let mut different = body;
        different["title"] = json!("different");
        assert_eq!(
            call(&a, &first, "/events", Some(different)).await.0,
            StatusCode::CONFLICT
        );
        let db = a.db.connection.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM community_events", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[tokio::test]
    async fn only_current_admin_can_publish_community_events_and_health_is_private() {
        let (_dir, a, first, second, _) = setup();
        let mut body = draft();
        body["community"] = json!(true);
        body["invitees"] = json!([]);
        assert_eq!(
            call(&a, &first, "/events", Some(body.clone())).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&a, &first, "/admin/health", None).await.0,
            StatusCode::FORBIDDEN
        );
        a.db.grant_admin("audit-a@example.com").unwrap();
        assert_eq!(
            call(&a, &first, "/events", Some(body)).await.0,
            StatusCode::OK
        );
        let (status, health) = call(&a, &first, "/admin/health", None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(health.get("connections").is_some());
        assert!(!health.to_string().contains("example.com"));
        assert_eq!(
            call(&a, &second, "/events", None).await.1["items"][0]["community"],
            true
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=0 WHERE id=1", [])
            .unwrap();
        assert_eq!(
            call(&a, &first, "/admin/health", None).await.0,
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn cancellation_bans_revocation_and_origin_checks_are_enforced() {
        let (_dir, a, first, second, _) = setup();
        let (_, created) = call(&a, &first, "/events", Some(draft())).await;
        let path = format!("/events/{}", created["id"].as_str().unwrap());
        let mut wrong = first.clone();
        wrong.insert("origin", "https://other.invalid".parse().unwrap());
        assert_eq!(
            call(&a, &wrong, &path, Some(json!({"action":"cancel"})))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&a, &first, &path, Some(json!({"action":"cancel"})))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            call(&a, &second, &path, Some(json!({"action":"attend"})))
                .await
                .0,
            StatusCode::CONFLICT
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=-1 WHERE id=2", [])
            .unwrap();
        assert_eq!(
            call(&a, &second, "/events", None).await.0,
            StatusCode::UNAUTHORIZED
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE user_id=1", [])
            .unwrap();
        assert_eq!(
            call(&a, &first, "/events", None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn non_friends_and_unknown_servers_are_rejected() {
        let (_dir, a, first, _, _) = setup();
        let mut body = draft();
        body["invitees"] = json!([3]);
        assert_eq!(
            call(&a, &first, "/events", Some(body.clone())).await.0,
            StatusCode::BAD_REQUEST
        );
        body["invitees"] = json!([2]);
        body["server"] = json!("https://127.0.0.1");
        assert_eq!(
            call(&a, &first, "/events", Some(body)).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    #[tokio::test]
    async fn acknowledged_event_can_be_retried_after_start_window_and_cleanup_cascades() {
        let (_dir, a, first, _, _) = setup();
        let mut body = draft();
        let (_, one) = call(&a, &first, "/events", Some(body.clone())).await;
        body["starts"] = json!(now() - 3600);
        body["ends"] = json!(now() + 3600);
        {
            let db = a.db.connection.lock().unwrap();
            // Simulate time having passed since an identical request was committed.
            db.execute(
                "UPDATE community_events SET request_hash=?",
                [crate::db::digest(
                    &serde_json::to_string(&serde_json::from_value::<Draft>(body.clone()).unwrap())
                        .unwrap(),
                )],
            )
            .unwrap();
        }
        assert_eq!(call(&a, &first, "/events", Some(body.clone())).await.1, one);
        body["request_id"] = json!("22345678-1234-1234-1234-123456789abc");
        assert_eq!(
            call(&a, &first, "/events", Some(body)).await.0,
            StatusCode::BAD_REQUEST
        );
        let db = a.db.connection.lock().unwrap();
        db.execute("UPDATE community_events SET ends=?", [now() - 31 * 86400])
            .unwrap();
        cleanup(&db).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM community_events", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM community_invites", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn drafts_reject_unbounded_input_and_invalid_time() {
        for (field, value) in [
            ("title", json!("x".repeat(241))),
            ("description", json!("x".repeat(4001))),
            ("starts", json!(now())),
            ("ends", json!(now() + 99 * 86400)),
            ("channel", json!(9_007_199_254_740_992u64)),
            ("invitees", json!([2, 2])),
            ("invitees", json!(vec![2; 65])),
        ] {
            let mut body = draft();
            body[field] = value;
            assert!(
                {
                    let draft = serde_json::from_value::<Draft>(body).unwrap();
                    draft.validate().is_err() || draft.validate_time().is_err()
                },
                "{field}"
            );
        }
    }
}
