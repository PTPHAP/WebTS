//! Reply notifications are references to visible comments, not copies of their bodies.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyPage {
    account: i64,
    before: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadReplies {
    account: i64,
    through: i64,
}
// Personal notifications intentionally respect blocks even for moderators.
const FILTER: &str = "FROM community_items p INDEXED BY community_author_listing CROSS JOIN community_items c INDEXED BY community_reply_time ON p.id=c.parent JOIN users u ON u.id=c.owner WHERE p.owner=?1 AND p.kind='post' AND p.ready=1 AND p.deleted=0 AND c.kind='comment' AND c.owner!=?1 AND c.ready=1 AND c.deleted=0 AND c.created_at>=?2 AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?3 AND NOT EXISTS(SELECT 1 FROM friendships f WHERE f.lo=min(c.owner,?1) AND f.hi=max(c.owner,?1) AND f.status=2)";
fn summary(db: &Connection, s: &Session, enabled: bool) -> Api<Value> {
    session_valid(db, s)?;
    if !enabled {
        return Ok(json!({"enabled":false,"unread":0,"latest":0}));
    }
    let read: i64 = db
        .query_row(
            "SELECT COALESCE((SELECT through_seq FROM community_reply_reads WHERE user_id=?),0)",
            [s.user.id],
            |r| r.get(0),
        )
        .map_err(anyhow::Error::from)?;
    let (unread,latest):(i64,i64)=db.query_row(&format!("SELECT count(*),COALESCE(max(seq),0) FROM (SELECT c.seq {FILTER} AND c.seq>?4 ORDER BY c.seq DESC LIMIT 100)"),params![s.user.id,now()-30*86400,now(),read],|r|Ok((r.get(0)?,r.get(1)?))).map_err(anyhow::Error::from)?;
    Ok(json!({"enabled":true,"unread":unread,"latest":latest}))
}
pub(crate) fn reply_summary(a: &App, s: &Session) -> Api<Value> {
    let runtime = a.runtime.read().unwrap();
    let db = a.db.connection.lock().unwrap();
    summary(&db, s, runtime.settings.community_enabled)
}
pub async fn reply_list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<ReplyPage>,
) -> Api<Json<Value>> {
    if q.before.is_some_and(|v| v <= 0) {
        return Err(Error::bad("回复分页无效"));
    }
    app.work(move|a|{let s=a.session(&headers)?;scope(&s,q.account)?;let runtime=a.runtime.read().unwrap();let db=a.db.connection.lock().unwrap();let summary=summary(&db,&s,runtime.settings.community_enabled)?;
        if !runtime.settings.community_enabled {return Ok(Json(json!({"account":s.user.id,"enabled":false,"items":[],"next":null,"through":0,"summary":summary})));}
        let read:i64=db.query_row("SELECT COALESCE((SELECT through_seq FROM community_reply_reads WHERE user_id=?),0)",[s.user.id],|r|r.get(0)).map_err(anyhow::Error::from)?;
        let mut stmt=db.prepare(&format!("SELECT c.seq,c.id,c.parent,c.owner,c.created_at {FILTER} AND c.seq<?4 ORDER BY c.seq DESC LIMIT 21")).map_err(anyhow::Error::from)?;
        let rows=stmt.query_map(params![s.user.id,now()-30*86400,now(),q.before.unwrap_or(i64::MAX)],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
        let more=rows.len()>20;let next=rows.iter().take(20).next_back().map(|r|r.0);let mut items=vec![];
        for(seq,id,parent,owner,created_at)in rows.into_iter().take(20){let (name,avatar_hash)=author(&db,owner)?;
            let root=visible(&db,&s,&parent)?;let (post_name,post_avatar)=author(&db,root.owner)?;
            let (created,category):(i64,String)=db.query_row("SELECT created_at,category FROM community_items WHERE id=?",[&parent],|r|Ok((r.get(0)?,r.get(1)?))).map_err(anyhow::Error::from)?;
            let liked:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM community_likes WHERE item=? AND owner=?)",params![parent,s.user.id],|r|r.get(0)).map_err(anyhow::Error::from)?;
            let likes:i64=db.query_row("SELECT count(*) FROM community_likes WHERE item=?",[&parent],|r|r.get(0)).map_err(anyhow::Error::from)?;
            items.push(json!({"seq":seq,"id":id,"actor":name,"owner":owner,"avatar_hash":avatar_hash,"created_at":created_at,"read":seq<=read,"post":{"id":parent,"owner":root.owner,"name":post_name,"avatar_hash":post_avatar,"kind":"post","parent":null,"category":category,"created_at":created,"liked":liked,"likes":likes,"can_delete":true,"can_moderate":administrator(&db,&s)?,"state":changes::state(&db,&parent)?}}));
        }
        // The read boundary is this response's unread snapshot, never a future counter.
        Ok(Json(json!({"account":s.user.id,"enabled":true,"items":items,"next":if more{next}else{None},"through":summary["latest"],"summary":summary})))
    }).await
}
pub async fn reply_read(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<ReadReplies>,
) -> Api<Json<Value>> {
    if body.through < 0 {
        return Err(Error::bad("回复已读位置无效"));
    }
    app.work(move|a|{let s=a.session(&headers)?;scope(&s,body.account)?;let runtime=a.runtime.read().unwrap();if !runtime.settings.community_enabled{return Err(Error(StatusCode::FORBIDDEN,"社区动态当前关闭"));}let db=a.db.connection.lock().unwrap();let tx=db.unchecked_transaction().map_err(anyhow::Error::from)?;let previous=summary(&tx,&s,true)?;
        let newest=previous["latest"].as_i64().unwrap_or(0);let through=body.through.min(newest);
        tx.execute("INSERT INTO community_reply_reads(user_id,through_seq) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET through_seq=max(through_seq,excluded.through_seq)",params![s.user.id,through]).map_err(anyhow::Error::from)?;
        let result=summary(&tx,&s,true)?;tx.commit().map_err(anyhow::Error::from)?;Ok(Json(json!({"account":s.user.id,"summary":result})))
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seed(a: &App, owner: i64, kind: &str, parent: Option<&str>, n: u32) -> String {
        let id = format!("00000000-0000-4000-8000-{n:012x}");
        a.db.connection.lock().unwrap().execute("INSERT INTO community_items(id,owner,kind,parent,object_key,store_id,request_hash,created_at,ready) VALUES(?,?,?,?,?,'default','synthetic-reply-index',?,1)",params![id,owner,kind,parent,token().unwrap(),now()]).unwrap();
        id
    }
    fn enable(a: &App) {
        a.runtime.write().unwrap().settings.community_enabled = true;
    }
    fn seq(a: &App, id: &str) -> i64 {
        a.db.connection
            .lock()
            .unwrap()
            .query_row("SELECT seq FROM community_items WHERE id=?", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }
    #[tokio::test]
    async fn replies_are_scoped_to_own_posts_without_reading_or_copying_object_bodies() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let root = seed(&a, 1, "post", None, 1);
        let reply = seed(&a, 2, "comment", Some(&root), 2);
        seed(&a, 1, "comment", Some(&root), 3);
        let other = seed(&a, 2, "post", None, 4);
        seed(&a, 1, "comment", Some(&other), 5);
        let result = reply_list(
            State(a.clone()),
            h.clone(),
            Query(ReplyPage {
                account: 1,
                before: None,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(result["summary"]["unread"], 1);
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
        assert_eq!(result["items"][0]["id"], reply);
        assert_eq!(result["items"][0]["post"]["id"], root);
        assert!(!result.to_string().contains("email"));
        assert!(!result.to_string().contains("object_key"));
        assert!(!result.to_string().contains("ciphertext"));
        assert_eq!(
            reply_list(
                State(a.clone()),
                h.clone(),
                Query(ReplyPage {
                    account: 2,
                    before: None
                })
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::CONFLICT
        );
        let session = a.session(&h).unwrap();
        a.runtime.write().unwrap().settings.community_enabled = false;
        assert_eq!(reply_summary(&a, &session).unwrap()["unread"], 0);
        assert_eq!(
            reply_list(
                State(a.clone()),
                h.clone(),
                Query(ReplyPage {
                    account: 1,
                    before: None
                })
            )
            .await
            .unwrap()
            .0["items"],
            json!([])
        );
        assert_eq!(
            reply_read(
                State(a),
                h,
                Json(ReadReplies {
                    account: 1,
                    through: 100
                })
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn read_watermark_is_monotonic_snapshot_bounded_and_cannot_hide_future_replies() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let root = seed(&a, 1, "post", None, 11);
        let first = seed(&a, 2, "comment", Some(&root), 12);
        let snapshot = seq(&a, &first);
        let second = seed(&a, 2, "comment", Some(&root), 13);
        let read = |through| ReadReplies {
            account: 1,
            through,
        };
        let r = reply_read(State(a.clone()), h.clone(), Json(read(snapshot)))
            .await
            .unwrap()
            .0;
        assert_eq!(r["summary"]["unread"], 1);
        let r = reply_read(State(a.clone()), h.clone(), Json(read(snapshot)))
            .await
            .unwrap()
            .0;
        assert_eq!(r["summary"]["unread"], 1);
        assert_eq!(
            reply_read(
                State(a.clone()),
                h.clone(),
                Json(ReadReplies {
                    account: 2,
                    through: i64::MAX
                })
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::CONFLICT
        );
        let r = reply_read(State(a.clone()), h.clone(), Json(read(i64::MAX)))
            .await
            .unwrap()
            .0;
        assert_eq!(r["summary"]["unread"], 0);
        let watermark =
            a.db.connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT through_seq FROM community_reply_reads WHERE user_id=1",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap();
        assert_eq!(watermark, seq(&a, &second));
        seed(&a, 2, "comment", Some(&root), 14);
        let session = a.session(&h).unwrap();
        assert_eq!(reply_summary(&a, &session).unwrap()["unread"], 1);
        let _ = reply_read(State(a.clone()), h, Json(read(0)))
            .await
            .unwrap();
        assert_eq!(reply_summary(&a, &session).unwrap()["unread"], 1);
    }
    #[tokio::test]
    async fn deleted_blocked_banned_unverified_or_expired_replies_disappear_even_for_admins() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let root = seed(&a, 1, "post", None, 21);
        let reply = seed(&a, 2, "comment", Some(&root), 22);
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=1 WHERE id=1", [])
            .unwrap();
        let session = a.session(&h).unwrap();
        assert_eq!(reply_summary(&a, &session).unwrap()["unread"], 1);
        {
            let db = a.db.connection.lock().unwrap();
            db.execute("INSERT INTO friendships(lo,hi,requester,status,blocked_by,created_at) VALUES(1,2,1,2,2,?)",[now()]).unwrap();
        }
        assert_eq!(reply_summary(&a, &session).unwrap()["unread"], 0);
        {
            let db = a.db.connection.lock().unwrap();
            db.execute("DELETE FROM friendships", []).unwrap();
            db.execute("UPDATE users SET banned_until=-1 WHERE id=2", [])
                .unwrap();
        }
        assert_eq!(reply_summary(&a, &session).unwrap()["unread"], 0);
        {
            let db = a.db.connection.lock().unwrap();
            db.execute("UPDATE users SET banned_until=0,verified=0 WHERE id=2", [])
                .unwrap();
        }
        assert_eq!(reply_summary(&a, &session).unwrap()["unread"], 0);
        {
            let db = a.db.connection.lock().unwrap();
            db.execute("UPDATE users SET verified=1 WHERE id=2", [])
                .unwrap();
            db.execute(
                "UPDATE community_items SET created_at=? WHERE id=?",
                params![now() - 31 * 86400, reply],
            )
            .unwrap();
        }
        assert_eq!(reply_summary(&a, &session).unwrap()["unread"], 0);
        {
            let db = a.db.connection.lock().unwrap();
            db.execute(
                "UPDATE community_items SET created_at=? WHERE id=?",
                params![now(), reply],
            )
            .unwrap();
            db.execute("UPDATE community_items SET deleted=1 WHERE id=?", [root])
                .unwrap();
        }
        assert_eq!(reply_summary(&a, &session).unwrap()["unread"], 0);
        a.db.connection
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE user_id=1", [])
            .unwrap();
        assert_eq!(
            reply_summary(&a, &session).err().unwrap().0,
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn reply_pagination_is_bounded_and_unread_count_saturates_without_global_user_history() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let root = seed(&a, 1, "post", None, 31);
        for n in 100..205 {
            seed(&a, 2, "comment", Some(&root), n);
        }
        let first = reply_list(
            State(a.clone()),
            h.clone(),
            Query(ReplyPage {
                account: 1,
                before: None,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(first["summary"]["unread"], 100);
        assert_eq!(first["items"].as_array().unwrap().len(), 20);
        let second = reply_list(
            State(a),
            h,
            Query(ReplyPage {
                account: 1,
                before: first["next"].as_i64(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(second["items"].as_array().unwrap().len(), 20);
        assert!(first["items"].as_array().unwrap().iter().all(|a| {
            second["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|b| a["id"] != b["id"])
        }));
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    #[tokio::test]
    async fn inbox_reuses_reply_summary_and_read_route_enforces_origin_account_schema_and_session()
    {
        let (_dir, a, h) = crate::app::tests::fixture();
        a.runtime.write().unwrap().settings.community_enabled = true;
        {
            let db = a.db.connection.lock().unwrap();
            let root = "00000000-0000-4000-8000-000000000501";
            db.execute("INSERT INTO community_items(id,owner,kind,object_key,store_id,request_hash,created_at,ready) VALUES(?,1,'post',?,'default','fixture',?,1)",params![root,token().unwrap(),now()]).unwrap();
            db.execute("INSERT INTO community_items(id,owner,kind,parent,object_key,store_id,request_hash,created_at,ready) VALUES(?,2,'comment',?,?,'default','fixture',?,1)",params!["00000000-0000-4000-8000-000000000502",root,token().unwrap(),now()]).unwrap();
        }
        let router = crate::app::router(a.clone());
        for (cookie, origin, body, status) in [
            (
                false,
                true,
                json!({"account":1,"through":2}),
                StatusCode::UNAUTHORIZED,
            ),
            (
                true,
                false,
                json!({"account":1,"through":2}),
                StatusCode::FORBIDDEN,
            ),
            (
                true,
                true,
                json!({"account":2,"through":2}),
                StatusCode::CONFLICT,
            ),
            (
                true,
                true,
                json!({"account":1,"through":2,"all_users":true}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                true,
                true,
                json!({"account":1,"through":-1}),
                StatusCode::BAD_REQUEST,
            ),
        ] {
            let mut request = Request::builder()
                .method("POST")
                .uri("/api/community/replies/read")
                .header("content-type", "application/json");
            if cookie {
                request = request.header("cookie", h["cookie"].clone());
            }
            if origin {
                request = request.header("origin", h["origin"].clone());
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::from(body.to_string())).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status);
        }
        let get = || {
            Request::builder()
                .uri("/api/notices")
                .header("cookie", h["cookie"].clone())
                .body(Body::empty())
                .unwrap()
        };
        let response = router.clone().oneshot(get()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 100000).await.unwrap()).unwrap();
        assert_eq!(body["account"], 1);
        assert_eq!(body["community"]["unread"], 1);
        let _ = reply_read(
            State(a.clone()),
            h.clone(),
            Json(ReadReplies {
                account: 1,
                through: i64::MAX,
            }),
        )
        .await
        .unwrap();
        let response = router.clone().oneshot(get()).await.unwrap();
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 100000).await.unwrap()).unwrap();
        assert_eq!(body["community"]["unread"], 0);
        a.db.connection
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE user_id=1", [])
            .unwrap();
        let response = router.oneshot(get()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[cfg(test)]
mod query_plan_tests {
    use super::*;
    #[tokio::test]
    async fn replies_start_from_recipient_posts_and_use_bounded_comment_time_ranges() {
        let (_dir, a, _h) = crate::app::tests::fixture();
        let db = a.db.connection.lock().unwrap();
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT c.seq {FILTER} AND c.seq>?4 ORDER BY c.seq DESC LIMIT 100"
        );
        let mut stmt = db.prepare(&sql).unwrap();
        let plan = stmt
            .query_map(params![1, now() - 30 * 86400, now(), 0], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            plan.iter()
                .any(|p| p.contains("SEARCH p USING INDEX community_author_listing")),
            "recipient access plan missing: {plan:?}"
        );
        assert!(
            plan.iter()
                .any(|p| p.contains("SEARCH c USING INDEX community_reply_time")),
            "bounded comment access missing: {plan:?}"
        );
        assert!(
            !plan
                .iter()
                .any(|p| p.contains("SEARCH c USING INDEX community_cleanup")),
            "global cleanup scan would starve unrelated work: {plan:?}"
        );
    }
}
