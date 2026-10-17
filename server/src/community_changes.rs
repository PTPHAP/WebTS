//! Versioned community writes. Pending uploads and retired objects keep durable bucket bindings.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    account: i64,
    id: String,
    revision: i64,
    issued_at: i64,
    payload: Payload,
    #[serde(default)]
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    account: i64,
    revision: i64,
    pinned: bool,
    locked: bool,
}
pub(super) fn state(db: &Connection, id: &str) -> Api<Value> {
    db.query_row("SELECT revision,updated_at,pinned,locked FROM community_items WHERE id=?",[id],|r|Ok(json!({"revision":r.get::<_,i64>(0)?,"edited_at":r.get::<_,i64>(1)?,"pinned":r.get::<_,bool>(2)?,"locked":r.get::<_,bool>(3)?}))).map_err(|e|anyhow::Error::from(e).into())
}
pub(super) fn comment_allowed(db: &Connection, s: &Session, id: &str) -> Api<()> {
    let locked: bool = db
        .query_row(
            "SELECT locked FROM community_items WHERE id=? AND kind='post'",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(anyhow::Error::from)?
        .ok_or_else(missing)?;
    if locked && !administrator(db, s)? {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "帖子已锁定，暂不接受评论或编辑",
        ));
    }
    Ok(())
}
pub async fn control(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Control>,
) -> Api<Json<Value>> {
    app.work(move|a|{
        let s=a.session(&headers)?;scope(&s,body.account)?;
        let db=a.db.connection.lock().unwrap();
        if !administrator(&db,&s)? {return Err(Error(StatusCode::FORBIDDEN,"仅管理员可置顶或锁帖"));}
        if visible(&db,&s,&id)?.kind!="post" {return Err(missing());}
        let tx=db.unchecked_transaction().map_err(anyhow::Error::from)?;
        let version:i64=tx.query_row("SELECT revision FROM community_items WHERE id=?",[&id],|r|r.get(0)).map_err(anyhow::Error::from)?;
        if version!=body.revision {return Err(conflict());}
        if body.pinned {let count:i64=tx.query_row("SELECT count(*) FROM community_items WHERE kind='post' AND pinned=1 AND deleted=0 AND id!=?",[&id],|r|r.get(0)).map_err(anyhow::Error::from)?;if count>=5{return Err(Error::bad("全站最多置顶5条帖子，请先取消已有置顶"));}}
        tx.execute("UPDATE community_items SET pinned=?,locked=?,revision=revision+1 WHERE id=?",params![body.pinned,body.locked,id]).map_err(anyhow::Error::from)?;
        tx.commit().map_err(anyhow::Error::from)?;
        Ok(Json(json!({"account":s.user.id,"id":id,"revision":version+1})))
    }).await
}
fn conflict() -> Error {
    Error(
        StatusCode::CONFLICT,
        "内容或分区已变化，请刷新核对后重新编辑",
    )
}
fn lease(kind: &str, target: &str) -> String {
    if kind == "edit" {
        format!("community:{target}")
    } else {
        format!("community-category:{target}")
    }
}
fn allowed(db: &Connection, s: &Session, kind: &str, target: &str) -> Api<i64> {
    if kind == "edit" {
        let row = visible(db, s, target)?;
        if row.kind != "post" || row.owner != s.user.id {
            return Err(missing());
        }
        comment_allowed(db, s, target)?;
        db.query_row(
            "SELECT revision FROM community_items WHERE id=?",
            [target],
            |r| r.get(0),
        )
        .map_err(|e| anyhow::Error::from(e).into())
    } else {
        if !administrator(db, s)? {
            return Err(Error(StatusCode::FORBIDDEN, "仅管理员可更换分区图标"));
        }
        if !categories(db)?.iter().any(|c| c.id == target) {
            return Err(missing());
        }
        db.query_row(
            "SELECT revision FROM community_category_revision WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| anyhow::Error::from(e).into())
    }
}
struct Prepared {
    key: String,
    config: Option<storage::Storage>,
    sealed: Vec<u8>,
    done: bool,
}
fn prepare(a: &App, h: &HeaderMap, kind: &str, target: &str, body: &mut Change) -> Api<Prepared> {
    if !uuid(&body.id)
        || body.revision < 0
        || body.password.len() > 128
        || (target != "general" && !uuid(target))
    {
        return Err(Error::bad("编辑编号或版本无效"));
    }
    let s = if kind == "icon" {
        crate::settings::admin(a, h)?;
        a.reauthenticate(h, &Zeroizing::new(std::mem::take(&mut body.password)))?
    } else {
        a.session(h)?
    };
    scope(&s, body.account)?;
    if kind == "edit" {
        let mut draft = Draft {
            account: body.account,
            id: target.into(),
            kind: post(),
            parent: None,
            issued_at: body.issued_at,
            payload: std::mem::take(&mut body.payload),
            category: general(),
        };
        validate(&mut draft)?;
        body.payload = draft.payload;
    } else {
        if !body.payload.title.is_empty() || !body.payload.text.is_empty() {
            return Err(Error::bad("分区图标仅接受图片"));
        }
        crate::site::image(&mut body.payload.image)?;
    }
    let binding = Zeroizing::new(
        serde_json::to_vec(&(kind, target, body.revision, body.issued_at, &body.payload))
            .map_err(anyhow::Error::from)?,
    );
    let hash = a.vault.request_tag("community-change-v1", &binding);
    let runtime = a.runtime.read().unwrap();
    if kind == "edit" && !runtime.settings.community_enabled {
        return Err(Error(StatusCode::FORBIDDEN, "社区动态当前关闭"));
    }
    let db = a.db.connection.lock().unwrap();
    let revision = allowed(&db, &s, kind, target)?;
    let old:Option<(i64,String,String,String,bool,bool)>=db.query_row("SELECT owner,request_hash,object_key,store_id,applied,cleaned FROM community_changes WHERE id=?",[&body.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(anyhow::Error::from)?;
    let domain = if kind == "edit" {
        format!("community-v2:{}", body.id)
    } else {
        format!("community-icon-v1:{}", body.id)
    };
    let owner = if kind == "edit" { s.user.id } else { 0 };
    let seal = || {
        a.vault
            .seal(
                owner,
                target,
                &domain,
                &Zeroizing::new(serde_json::to_vec(&body.payload).map_err(anyhow::Error::from)?),
            )
            .map_err(Error::from)
    };
    if let Some((owner, previous, key, store, applied, cleaned)) = old {
        if owner != s.user.id || hash != previous {
            return Err(conflict());
        }
        if applied {
            return Ok(Prepared {
                key: String::new(),
                config: None,
                sealed: vec![],
                done: true,
            });
        }
        if cleaned || revision != body.revision {
            return Err(conflict());
        }
        return Ok(Prepared {
            config: if key.is_empty() {
                None
            } else {
                Some(storage::resolve(&runtime.settings, &store)?)
            },
            key,
            sealed: seal()?,
            done: false,
        });
    }
    if revision != body.revision {
        return Err(conflict());
    }
    if body.issued_at < now() - 3600 || body.issued_at > now() + 300 {
        return Err(Error::bad("编辑时间已过期，请刷新重试"));
    }
    let (total,daily):(i64,i64)=db.query_row("SELECT (SELECT count(*) FROM community_changes),(SELECT count(*) FROM community_changes WHERE owner=? AND created_at>?)",params![s.user.id,now()-86400],|r|Ok((r.get(0)?,r.get(1)?))).map_err(anyhow::Error::from)?;
    if total >= 30000 || daily >= 100 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "编辑容量或今日编辑次数已达上限",
        ));
    }
    let clear = kind == "icon" && body.payload.image.is_empty();
    let (store, config) = if clear {
        (String::new(), None)
    } else {
        let (id, c) = storage::selected(&runtime.settings)?;
        if !c.enabled {
            return Err(Error(StatusCode::SERVICE_UNAVAILABLE, "请先启用对象存储"));
        }
        (id, Some(c))
    };
    let key = if clear { String::new() } else { token()? };
    let sealed = seal()?;
    db.execute("INSERT INTO community_changes(id,target,kind,owner,base_revision,request_hash,object_key,store_id,created_at) VALUES(?,?,?,?,?,?,?,?,?)",params![body.id,target,kind,s.user.id,body.revision,hash,key,store,now()]).map_err(anyhow::Error::from)?;
    Ok(Prepared {
        key,
        config,
        sealed,
        done: false,
    })
}
fn finish(a: &App, h: &HeaderMap, kind: &str, target: &str, body: &Change) -> Api<Json<Value>> {
    let s = a.session(h)?;
    scope(&s, body.account)?;
    let runtime = a.runtime.read().unwrap();
    if kind == "edit" && !runtime.settings.community_enabled {
        return Err(Error(StatusCode::FORBIDDEN, "社区动态当前关闭"));
    }
    let db = a.db.connection.lock().unwrap();
    let tx = db.unchecked_transaction().map_err(anyhow::Error::from)?;
    let revision = allowed(&tx, &s, kind, target)?;
    let row:Option<(String,String,bool,bool,i64)>=tx.query_row("SELECT object_key,store_id,applied,cleaned,base_revision FROM community_changes WHERE id=? AND target=? AND kind=? AND owner=?",params![body.id,target,kind,s.user.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(anyhow::Error::from)?;
    let (key, store, applied, cleaned, base) = row.ok_or_else(missing)?;
    if applied {
        return Ok(Json(
            json!({"account":s.user.id,"id":target,"request_id":body.id,"revision":base+1}),
        ));
    }
    if cleaned || base != body.revision || revision != base {
        return Err(conflict());
    }
    let old: (String, String) = if kind == "edit" {
        let old = tx
            .query_row(
                "SELECT object_key,store_id FROM community_items WHERE id=?",
                [target],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(anyhow::Error::from)?;
        tx.execute("UPDATE community_items SET object_key=?,store_id=?,content_version=?,revision=revision+1,updated_at=? WHERE id=?",params![key,store,body.id,now(),target]).map_err(anyhow::Error::from)?;
        old
    } else {
        let old = tx
            .query_row(
                "SELECT object_key,store_id FROM community_icons WHERE category=?",
                [target],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(anyhow::Error::from)?
            .unwrap_or_default();
        if key.is_empty() {
            tx.execute("DELETE FROM community_icons WHERE category=?", [target])
                .map_err(anyhow::Error::from)?;
        } else {
            tx.execute("INSERT INTO community_icons(category,object_key,store_id,version) VALUES(?,?,?,?) ON CONFLICT(category) DO UPDATE SET object_key=excluded.object_key,store_id=excluded.store_id,version=excluded.version",params![target,key,store,body.id]).map_err(anyhow::Error::from)?;
        }
        tx.execute(
            "UPDATE community_category_revision SET revision=revision+1 WHERE id=1",
            [],
        )
        .map_err(anyhow::Error::from)?;
        old
    };
    // After the atomic pointer swap this row owns the retired object. A retry of an
    // applied request never PUTs again, including after its old object was cleaned.
    tx.execute("UPDATE community_changes SET applied=1,object_key=?,store_id=?,cleaned=?,next_attempt=0 WHERE id=?",params![old.0,old.1,old.0.is_empty(),body.id]).map_err(anyhow::Error::from)?;
    tx.commit().map_err(anyhow::Error::from)?;
    Ok(Json(
        json!({"account":s.user.id,"id":target,"request_id":body.id,"revision":base+1}),
    ))
}
async fn change(
    app: Arc<App>,
    headers: HeaderMap,
    target: String,
    mut body: Change,
    kind: &'static str,
) -> Api<Json<Value>> {
    let permit = app.object_permit(&headers).await?;
    let transfer = app
        .friend_transfer(&lease(kind, &target))
        .try_lock_owned()
        .map_err(|_| Error(StatusCode::CONFLICT, "内容正在传输或清理，请稍后重试"))?;
    tokio::spawn(async move {
        let _transfer = transfer;
        let _permit = permit;
        let h = headers.clone();
        let t = target.clone();
        let (body, write) = app
            .expensive(move |a| {
                let write = prepare(a, &h, kind, &t, &mut body)?;
                Ok((body, write))
            })
            .await?;
        if !write.done
            && let Some(config) = write.config
        {
            storage::community_object(&config, "PUT", &write.key, write.sealed)
                .await
                .map_err(|e| {
                    Error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        storage::failure_message(&e),
                    )
                })?;
        }
        app.work(move |a| finish(a, &headers, kind, &target, &body))
            .await
    })
    .await
    .map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "编辑任务中断，请刷新核对后重试",
        )
    })?
}
pub async fn edit(
    State(app): State<Arc<App>>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Change>,
) -> Api<Json<Value>> {
    change(app, h, id, body, "edit").await
}
pub async fn set_icon(
    State(app): State<Arc<App>>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Change>,
) -> Api<Json<Value>> {
    change(app, h, id, body, "icon").await
}

pub async fn icon(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<AvatarQuery>,
) -> Api<Response> {
    if !uuid(&q.v) {
        return Err(Error::bad("图标版本无效"));
    }
    let _permit = app.object_permit(&headers).await?;
    let h = headers.clone();
    let target = id.clone();
    let version = q.v.clone();
    let (key,config)=app.work(move|a|{let s=a.session(&h)?;scope(&s,q.account)?;let runtime=a.runtime.read().unwrap();let db=a.db.connection.lock().unwrap();allow_read(&runtime.settings,&db,&s)?;let row:Option<(String,String)>=db.query_row("SELECT object_key,store_id FROM community_icons WHERE category=? AND version=?",params![target,version],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(anyhow::Error::from)?;let (key,store)=row.ok_or_else(missing)?;Ok((key,storage::resolve(&runtime.settings,&store)?))}).await?;
    let bytes = storage::community_object(&config, "GET", &key, vec![])
        .await
        .map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "图标暂不可读取"))?;
    app.work(move |a| icon_complete(a, &headers, &id, &q, &key, &bytes))
        .await
}
fn icon_complete(
    a: &App,
    headers: &HeaderMap,
    id: &str,
    q: &AvatarQuery,
    key: &str,
    bytes: &[u8],
) -> Api<Response> {
    let s = a.session(headers)?;
    scope(&s, q.account)?;
    let runtime = a.runtime.read().unwrap();
    let db = a.db.connection.lock().unwrap();
    allow_read(&runtime.settings, &db, &s)?;
    let same:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM community_icons WHERE category=? AND version=? AND object_key=?)",params![id,q.v,key],|r|r.get(0)).map_err(anyhow::Error::from)?;
    if !same {
        return Err(conflict());
    }
    let plain = a
        .vault
        .open(0, id, &format!("community-icon-v1:{}", q.v), bytes)?;
    let payload: Payload = serde_json::from_slice(&plain).map_err(anyhow::Error::from)?;
    let encoded = payload
        .image
        .strip_prefix("data:image/jpeg;base64,")
        .ok_or_else(missing)?;
    if encoded.len() > 175000 {
        return Err(missing());
    }
    let image = STANDARD.decode(encoded).map_err(|_| missing())?;
    if image.len() > 128 * 1024
        || image::guess_format(&image).ok() != Some(image::ImageFormat::Jpeg)
    {
        return Err(missing());
    }
    Ok((
        [
            ("content-type", "image/jpeg"),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        image,
    )
        .into_response())
}
pub(super) async fn cleanup(app: &Arc<App>) {
    let rows=app.work(|a|{let db=a.db.connection.lock().unwrap();db.execute("DELETE FROM community_changes WHERE cleaned=1 AND created_at<?",[now()-30*86400]).map_err(anyhow::Error::from)?;let mut stmt=db.prepare("SELECT id,target,kind FROM community_changes WHERE cleaned=0 AND (applied=1 OR created_at<=?) AND next_attempt<=? ORDER BY next_attempt,created_at LIMIT 2").map_err(anyhow::Error::from)?;let rows=stmt.query_map(params![now()-3600,now()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;Ok(rows)}).await;
    if let Ok(rows) = rows {
        for (id, target, kind) in rows {
            let _ = clean_with(app, id, target, kind, |c, k| async move {
                storage::community_remove(&c, &k).await
            })
            .await;
        }
    }
}
async fn clean_with<F, Fut>(
    app: &Arc<App>,
    id: String,
    target: String,
    kind: String,
    remove: F,
) -> Api<()>
where
    F: FnOnce(storage::Storage, String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let Ok(permit) = app.friend_objects.clone().try_acquire_owned() else {
        return Ok(());
    };
    let Ok(transfer) = app.friend_transfer(&lease(&kind, &target)).try_lock_owned() else {
        return Ok(());
    };
    // Own the permits beyond HTTP lifetime and never clean a currently referenced key.
    let mid = id.clone();
    let row=app.work(move|a|{let runtime=a.runtime.read().unwrap();let db=a.db.connection.lock().unwrap();let row:Option<(String,String)>=db.query_row("SELECT object_key,store_id FROM community_changes WHERE id=? AND cleaned=0 AND (applied=1 OR created_at<=?) AND next_attempt<=? AND NOT EXISTS(SELECT 1 FROM community_items i WHERE i.object_key=community_changes.object_key AND i.cleaned=0) AND NOT EXISTS(SELECT 1 FROM community_icons i WHERE i.object_key=community_changes.object_key)",params![mid,now()-3600,now()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(anyhow::Error::from)?;row.map(|(key,store)|if key.is_empty(){Ok((key,None))}else{Ok((key,Some(storage::resolve(&runtime.settings,&store)?)))}).transpose()}).await?;
    let Some((key, config)) = row else {
        return Ok(());
    };
    let ok = match config {
        Some(c) => remove(c, key).await.is_ok(),
        None => true,
    };
    app.work(move|a|{let _permit=permit;let _transfer=transfer;let db=a.db.connection.lock().unwrap();if ok{db.execute("UPDATE community_changes SET cleaned=1 WHERE id=?",[id]).map_err(anyhow::Error::from)?;}else{db.execute("UPDATE community_changes SET attempts=min(attempts+1,8),next_attempt=?+min(3600,30*(1<<min(attempts,7))) WHERE id=?",params![now(),id]).map_err(anyhow::Error::from)?;}Ok(())}).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn enable(a: &App) {
        let mut rt = a.runtime.write().unwrap();
        rt.settings.community_enabled = true;
        rt.settings.storage = storage::Storage {
            enabled: true,
            endpoint: "https://s3.example.invalid".into(),
            bucket: "original".into(),
            region: "us-east-1".into(),
            access_key: "synthetic-access".into(),
            secret_key: "synthetic-secret".into(),
            ..Default::default()
        };
    }
    fn draft() -> Draft {
        Draft {
            account: 1,
            id: "00000000-0000-4000-8000-000000000101".into(),
            kind: post(),
            parent: None,
            issued_at: now(),
            payload: Payload {
                title: "original".into(),
                text: "old payload".into(),
                image: String::new(),
            },
            category: general(),
        }
    }
    fn edit(n: u32, revision: i64) -> Change {
        Change {
            account: 1,
            id: format!("00000000-0000-4000-8000-{n:012x}"),
            revision,
            issued_at: now(),
            payload: Payload {
                title: "edited".into(),
                text: "literal <script>still text</script>".into(),
                image: String::new(),
            },
            password: String::new(),
        }
    }
    fn ready(a: &App, h: &HeaderMap) -> (Draft, Write) {
        let mut d = draft();
        let w = super::super::prepare(a, h, &mut d).unwrap();
        let _ = super::super::finish(a, h, &d).unwrap();
        (d, w)
    }
    fn status<T>(r: Api<T>) -> StatusCode {
        r.err().unwrap().0
    }
    fn admin(a: &App) {
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=1 WHERE id=1", [])
            .unwrap();
    }
    fn peer(a: &App, h: &HeaderMap) -> HeaderMap {
        let token = a.db.create_session(2, false, "fixture hash").unwrap();
        let mut other = h.clone();
        other.insert("cookie", format!("webts_dev={token}").parse().unwrap());
        other
    }
    #[tokio::test]
    async fn editing_atomically_replaces_ciphertext_and_retries_never_restore_original_content() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let (d, old) = ready(&a, &h);
        let mut e = edit(201, 0);
        let w = prepare(&a, &h, "edit", &d.id, &mut e).unwrap();
        assert_ne!(w.key, old.key);
        assert!(!w.sealed.windows(6).any(|b| b == b"edited"));
        // Until commit old content is readable and pending object has a durable binding.
        assert_eq!(
            read_version(&a, &h, &d.id, &old.sealed, Some(&old.key))
                .unwrap()
                .0["content"]["title"],
            "original"
        );
        let _ = finish(&a, &h, "edit", &d.id, &e).unwrap();
        assert_eq!(
            status(read_version(&a, &h, &d.id, &old.sealed, Some(&old.key))),
            StatusCode::CONFLICT
        );
        let r = read_version(&a, &h, &d.id, &w.sealed, Some(&w.key))
            .unwrap()
            .0;
        assert_eq!(r["content"]["title"], "edited");
        assert_eq!(r["state"]["revision"], 1);
        assert!(
            read_version(&a, &h, &d.id, &old.sealed, Some(&w.key)).is_err(),
            "old ciphertext must not authenticate under new object version"
        );
        let stored: (String, String, bool) =
            a.db.connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT object_key,store_id,applied FROM community_changes WHERE id=?",
                    [&e.id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
        assert_eq!(stored, (old.key.clone(), "default".into(), true));
        assert!(prepare(&a, &h, "edit", &d.id, &mut e).unwrap().done);
        assert!(finish(&a, &h, "edit", &d.id, &e).is_ok());
        let mut original = d;
        let retry = super::super::prepare(&a, &h, &mut original).unwrap();
        assert!(retry.ready);
        assert_eq!(retry.key, w.key);
        let _ = super::super::finish(&a, &h, &original).unwrap();
        assert_eq!(
            read_version(&a, &h, &original.id, &w.sealed, Some(&w.key))
                .unwrap()
                .0["content"]["title"],
            "edited"
        );
    }
    #[tokio::test]
    async fn concurrent_edit_lock_delete_and_revoked_session_preserve_current_version() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let (d, _) = ready(&a, &h);
        let mut one = edit(211, 0);
        let mut two = edit(212, 0);
        prepare(&a, &h, "edit", &d.id, &mut one).unwrap();
        prepare(&a, &h, "edit", &d.id, &mut two).unwrap();
        let _ = finish(&a, &h, "edit", &d.id, &one).unwrap();
        assert_eq!(
            status(finish(&a, &h, "edit", &d.id, &two)),
            StatusCode::CONFLICT
        );
        let mut late = edit(213, 1);
        prepare(&a, &h, "edit", &d.id, &mut late).unwrap();
        admin(&a);
        let _ = control(
            State(a.clone()),
            h.clone(),
            Path(d.id.clone()),
            Json(Control {
                account: 1,
                revision: 1,
                pinned: false,
                locked: true,
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            status(finish(&a, &h, "edit", &d.id, &late)),
            StatusCode::CONFLICT
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=0 WHERE id=1", [])
            .unwrap();
        let mut locked = edit(214, 2);
        assert_eq!(
            status(prepare(&a, &h, "edit", &d.id, &mut locked)),
            StatusCode::FORBIDDEN
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE community_items SET locked=0 WHERE id=?", [&d.id])
            .unwrap();
        prepare(&a, &h, "edit", &d.id, &mut locked).unwrap();
        a.db.connection
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE user_id=1", [])
            .unwrap();
        assert_eq!(
            status(finish(&a, &h, "edit", &d.id, &locked)),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn foreign_edits_hidden_targets_invalid_payload_and_changed_request_are_rejected() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let (d, _) = ready(&a, &h);
        let other = peer(&a, &h);
        let mut e = edit(221, 0);
        e.account = 2;
        assert_eq!(
            status(prepare(&a, &other, "edit", &d.id, &mut e)),
            StatusCode::NOT_FOUND
        );
        e.account = 1;
        prepare(&a, &h, "edit", &d.id, &mut e).unwrap();
        e.payload.text = "different retry".into();
        assert_eq!(
            status(prepare(&a, &h, "edit", &d.id, &mut e)),
            StatusCode::CONFLICT
        );
        let mut invalid = edit(222, 0);
        invalid.payload.image = "data:image/svg+xml;base64,aaaa".into();
        assert_eq!(
            status(prepare(&a, &h, "edit", &d.id, &mut invalid)),
            StatusCode::BAD_REQUEST
        );
        mark_deleted(&a.db.connection.lock().unwrap(), &d.id, 1).unwrap();
        assert_eq!(
            status(finish(&a, &h, "edit", &d.id, &e)),
            StatusCode::NOT_FOUND
        );
    }
    #[tokio::test]
    async fn locks_check_both_comment_prepare_and_commit_without_suppressing_reports() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let (d, _) = ready(&a, &h);
        let other = peer(&a, &h);
        let mut comment = Draft {
            account: 2,
            id: "00000000-0000-4000-8000-000000000231".into(),
            kind: "comment".into(),
            parent: Some(d.id.clone()),
            issued_at: now(),
            payload: Payload {
                title: String::new(),
                text: "in flight".into(),
                image: String::new(),
            },
            category: general(),
        };
        super::super::prepare(&a, &other, &mut comment).unwrap();
        admin(&a);
        let _ = control(
            State(a.clone()),
            h.clone(),
            Path(d.id.clone()),
            Json(Control {
                account: 1,
                revision: 0,
                pinned: true,
                locked: true,
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            status(super::super::finish(&a, &other, &comment)),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status(super::super::prepare(&a, &other, &mut comment)),
            StatusCode::FORBIDDEN
        );
        comment.kind = "report".into();
        comment.id = "00000000-0000-4000-8000-000000000232".into();
        super::super::prepare(&a, &other, &mut comment).unwrap();
        let _ = super::super::finish(&a, &other, &comment).unwrap();
        let session = a.session(&other).unwrap();
        assert!(visible(&a.db.connection.lock().unwrap(), &session, &d.id).is_ok());
        assert_eq!(
            status(
                control(
                    State(a.clone()),
                    other,
                    Path(d.id.clone()),
                    Json(Control {
                        account: 2,
                        revision: 1,
                        pinned: false,
                        locked: false
                    })
                )
                .await
            ),
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn retired_objects_retry_original_store_and_never_delete_current_object() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        let (d, old) = ready(&a, &h);
        let mut e = edit(241, 0);
        let w = prepare(&a, &h, "edit", &d.id, &mut e).unwrap();
        let _ = finish(&a, &h, "edit", &d.id, &e).unwrap();
        let id = e.id.clone();
        clean_with(
            &a,
            id.clone(),
            d.id.clone(),
            "edit".into(),
            |config, key| async move {
                assert_eq!(config.bucket, "original");
                assert_eq!(key, old.key);
                anyhow::bail!("synthetic delete failure")
            },
        )
        .await
        .unwrap();
        {
            let db = a.db.connection.lock().unwrap();
            let (n, c): (i64, bool) = db
                .query_row(
                    "SELECT attempts,cleaned FROM community_changes WHERE id=?",
                    [&id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!((n, c), (1, false));
            db.execute(
                "UPDATE community_changes SET next_attempt=0 WHERE id=?",
                [&id],
            )
            .unwrap();
        }
        let lease = a
            .friend_transfer(&format!("community:{}", d.id))
            .lock_owned()
            .await;
        clean_with(&a, id.clone(), d.id.clone(), "edit".into(), |_, _| async {
            panic!("active transfer must exclude cleanup")
        })
        .await
        .unwrap();
        drop(lease);
        let current = w.key.clone();
        clean_with(
            &a,
            id.clone(),
            d.id.clone(),
            "edit".into(),
            move |_, key| async move {
                assert_ne!(key, current);
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(prepare(&a, &h, "edit", &d.id, &mut e).unwrap().done);
        let db = a.db.connection.lock().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT object_key FROM community_items WHERE id=?",
                [&d.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            w.key
        );
        assert!(
            db.query_row(
                "SELECT cleaned FROM community_changes WHERE id=?",
                [id],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
        );
    }
    #[tokio::test]
    async fn icon_upload_clear_revision_password_and_role_checks_are_independent_of_names() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        admin(&a);
        let password = "synthetic icon password 123";
        let hash = crate::password::hash(password).unwrap();
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET password_hash=? WHERE id=1", [hash])
            .unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(20, 20)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        let image = format!(
            "data:image/jpeg;base64,{}",
            STANDARD.encode(out.into_inner())
        );
        let mut e = edit(251, 0);
        e.payload = Payload {
            image,
            title: String::new(),
            text: String::new(),
        };
        e.password = "wrong".into();
        assert_eq!(
            status(prepare(&a, &h, "icon", "general", &mut e)),
            StatusCode::UNAUTHORIZED
        );
        e.password = password.into();
        let w = prepare(&a, &h, "icon", "general", &mut e).unwrap();
        assert!(!w.key.is_empty());
        let _ = finish(&a, &h, "icon", "general", &e).unwrap();
        assert_eq!(
            categories(&a.db.connection.lock().unwrap()).unwrap()[0].icon,
            e.id
        );
        let mut clear = edit(252, 1);
        clear.payload = Payload::default();
        clear.password = password.into();
        let prepared = prepare(&a, &h, "icon", "general", &mut clear).unwrap();
        assert!(prepared.config.is_none());
        let _ = finish(&a, &h, "icon", "general", &clear).unwrap();
        assert!(
            categories(&a.db.connection.lock().unwrap()).unwrap()[0]
                .icon
                .is_empty()
        );
        let db = a.db.connection.lock().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT object_key FROM community_changes WHERE id=?",
                [&clear.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            w.key
        );
        db.execute("UPDATE users SET is_admin=0 WHERE id=1", [])
            .unwrap();
        drop(db);
        assert_eq!(
            status(finish(&a, &h, "icon", "general", &clear)),
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn pinning_is_bounded_and_does_not_consume_sequence_pagination() {
        let (_dir, a, h) = crate::app::tests::fixture();
        enable(&a);
        admin(&a);
        for n in 1..=19 {
            let mut d = draft();
            d.id = format!("00000000-0000-4000-8000-{n:012x}");
            super::super::prepare(&a, &h, &mut d).unwrap();
            let _ = super::super::finish(&a, &h, &d).unwrap();
            if n <= 5 {
                let _ = control(
                    State(a.clone()),
                    h.clone(),
                    Path(d.id),
                    Json(Control {
                        account: 1,
                        revision: 0,
                        pinned: true,
                        locked: false,
                    }),
                )
                .await
                .unwrap();
            }
        }
        let id = "00000000-0000-4000-8000-000000000006".to_string();
        assert_eq!(
            status(
                control(
                    State(a.clone()),
                    h.clone(),
                    Path(id),
                    Json(Control {
                        account: 1,
                        revision: 0,
                        pinned: true,
                        locked: false
                    })
                )
                .await
            ),
            StatusCode::BAD_REQUEST
        );
        let page = |before| Listing {
            author: None,
            latest: false,
            account: 1,
            before,
            parent: None,
            kind: post(),
            category: None,
        };
        let first = list(State(a.clone()), h.clone(), Query(page(None)))
            .await
            .unwrap()
            .0;
        assert_eq!(first["items"].as_array().unwrap().len(), 17);
        assert_eq!(first["items"][0]["state"]["pinned"], true);
        let second = list(State(a.clone()), h, Query(page(first["next"].as_i64())))
            .await
            .unwrap()
            .0;
        assert_eq!(second["items"].as_array().unwrap().len(), 2);
        assert!(
            second["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["state"]["pinned"] == false)
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[tokio::test]
    async fn pending_and_current_icons_count_as_objects_and_late_plaintext_rechecks_version_and_access()
     {
        use axum::body::to_bytes;
        let (_dir, a, h) = crate::app::tests::fixture();
        {
            let mut rt = a.runtime.write().unwrap();
            rt.settings.community_enabled = true;
        }
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=1 WHERE id=1", [])
            .unwrap();
        let key = "a".repeat(64);
        let version = "00000000-0000-4000-8000-000000000311".to_string();
        let mut picture = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(10, 10)
            .write_to(&mut picture, image::ImageFormat::Jpeg)
            .unwrap();
        let image = picture.into_inner();
        let sealed = a
            .vault
            .seal(
                0,
                "general",
                &format!("community-icon-v1:{version}"),
                &serde_json::to_vec(&Payload {
                    title: String::new(),
                    text: String::new(),
                    image: format!("data:image/jpeg;base64,{}", STANDARD.encode(&image)),
                })
                .unwrap(),
            )
            .unwrap();
        {
            let db = a.db.connection.lock().unwrap();
            db.execute(
                "INSERT INTO community_icons VALUES('general',?,'default',?)",
                params![key, version],
            )
            .unwrap();
            db.execute("INSERT INTO community_changes(id,target,kind,owner,base_revision,request_hash,object_key,store_id,created_at) VALUES(?,'general','icon',1,0,'fixture',?,'default',?)",params![version,"b".repeat(64),now()-4000]).unwrap();
        }
        let objects = storage::profiles(State(a.clone()), h.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(objects["profiles"][0]["objects"], 2);
        assert_eq!(objects["profiles"][0]["storage"]["objects"]["community"], 2);
        let q = AvatarQuery {
            account: 1,
            v: version.clone(),
        };
        let response = icon_complete(&a, &h, "general", &q, &key, &sealed).unwrap();
        assert_eq!(response.headers()["content-type"], "image/jpeg");
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
            image
        );
        let foreign = AvatarQuery {
            account: 2,
            v: version.clone(),
        };
        assert_eq!(
            icon_complete(&a, &h, "general", &foreign, &key, &sealed)
                .err()
                .unwrap()
                .0,
            StatusCode::CONFLICT
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE community_icons SET version=?",
                ["00000000-0000-4000-8000-000000000312"],
            )
            .unwrap();
        assert_eq!(
            icon_complete(&a, &h, "general", &q, &key, &sealed)
                .err()
                .unwrap()
                .0,
            StatusCode::CONFLICT
        );
        a.runtime.write().unwrap().settings.community_enabled = false;
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=0 WHERE id=1", [])
            .unwrap();
        assert_eq!(
            icon_complete(&a, &h, "general", &q, &key, &sealed)
                .err()
                .unwrap()
                .0,
            StatusCode::FORBIDDEN
        );
        let stale = a.session(&h).unwrap();
        a.db.connection
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE user_id=1", [])
            .unwrap();
        {
            let runtime = a.runtime.read().unwrap();
            let db = a.db.connection.lock().unwrap();
            assert_eq!(
                allow_read(&runtime.settings, &db, &stale).err().unwrap().0,
                StatusCode::UNAUTHORIZED
            );
        }

        assert_eq!(
            icon_complete(&a, &h, "general", &q, &key, &sealed)
                .err()
                .unwrap()
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn destructive_report_decision_requires_the_exact_reviewed_content_version() {
        let (_dir, a, h) = crate::app::tests::fixture();
        a.runtime.write().unwrap().settings.community_enabled = true;
        let target = "00000000-0000-4000-8000-000000000321".to_string();
        let report = "00000000-0000-4000-8000-000000000322".to_string();
        {
            let db = a.db.connection.lock().unwrap();
            db.execute("UPDATE users SET is_admin=1 WHERE id=1", [])
                .unwrap();
            for (id, kind, parent) in [(&target, "post", None), (&report, "report", Some(&target))]
            {
                db.execute("INSERT INTO community_items(id,owner,kind,parent,object_key,store_id,request_hash,created_at,ready) VALUES(?,1,?,?,?,'default','fixture',?,1)",params![id,kind,parent,token().unwrap(),now()]).unwrap();
            }
        }
        let body = |version| Action {
            account: 1,
            liked: false,
            remove: true,
            reviewed_revision: version,
        };
        assert_eq!(
            decide(
                State(a.clone()),
                h.clone(),
                Path(report.clone()),
                Json(body(None))
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::CONFLICT
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE community_items SET revision=1 WHERE id=?",
                [&target],
            )
            .unwrap();
        assert_eq!(
            decide(
                State(a.clone()),
                h.clone(),
                Path(report.clone()),
                Json(body(Some(0)))
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::CONFLICT
        );
        let _ = decide(State(a.clone()), h, Path(report), Json(body(Some(1))))
            .await
            .unwrap();
        assert!(
            a.db.connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT deleted FROM community_items WHERE id=?",
                    [target],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap()
        );
    }
    #[tokio::test]
    async fn mutation_routes_keep_origin_schema_account_role_and_body_limits() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        let (_dir, a, h) = crate::app::tests::fixture();
        let router = crate::app::router(a.clone());
        let target = "00000000-0000-4000-8000-000000000331";
        let paths = [
            format!("/api/community/{target}/edit"),
            format!("/api/admin/community/{target}/control"),
            "/api/admin/community/categories/general/icon".into(),
        ];
        for path in paths {
            let request = Request::builder()
                .method("POST")
                .uri(&path)
                .header("content-type", "application/json")
                .header("cookie", h["cookie"].clone())
                .header("origin", "https://evil.invalid")
                .body(Body::from("{}"))
                .unwrap();
            assert_eq!(
                router.clone().oneshot(request).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/admin/community/{target}/control"))
            .header("content-type", "application/json")
            .header("cookie", h["cookie"].clone())
            .header("origin", h["origin"].clone())
            .body(Body::from(
                json!({"account":1,"revision":0,"pinned":true,"locked":false}).to_string(),
            ))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let request=Request::builder().method("POST").uri(format!("/api/community/{target}/edit")).header("content-type","application/json").header("cookie",h["cookie"].clone()).header("origin",h["origin"].clone()).body(Body::from(json!({"account":1,"id":target,"revision":0,"issued_at":now(),"payload":{},"admin":true}).to_string())).unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let request = Request::builder()
            .method("POST")
            .uri("/api/admin/community/categories/general/icon")
            .header("content-type", "application/json")
            .header("cookie", h["cookie"].clone())
            .header("origin", h["origin"].clone())
            .body(Body::from("x".repeat(193 * 1024)))
            .unwrap();
        assert_eq!(
            router.oneshot(request).await.unwrap().status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
