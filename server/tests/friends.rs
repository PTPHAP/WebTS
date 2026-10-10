use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use web_ts::{
    app::{App, router},
    config::Config,
    password,
};
const ORIGIN: &str = "https://friends.example";
const PASSWORD: &str = "synthetic friends fixture password only";
#[tokio::test]
async fn account_preferences_merge_across_sessions_and_reject_secrets_and_cross_account_writes() {
    let (_root, app) = setup();
    let (ai, a) = user(&app, "settings-a@example.invalid");
    let (bi, b) = user(&app, "settings-b@example.invalid");
    assert_eq!(
        call(&app, "/preferences", None, &a, "", ORIGIN).await.1["initialized"],
        false
    );
    let change = |account, patch| json!({"account":account,"patch":patch});
    assert_eq!(
        call(
            &app,
            "/preferences",
            Some(change(ai, json!({"burn":true,"burnSeconds":600}))),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    let patch = |cookie: &str, body: Value| {
        Request::builder()
            .method("PATCH")
            .uri("/api/preferences")
            .header("cookie", cookie)
            .header("origin", ORIGIN)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    assert_eq!(
        router(app.clone())
            .oneshot(patch(
                &a,
                change(ai, json!({"burn":true,"burnSeconds":600}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let (_, second) = {
        let hash = app
            .db
            .password("settings-a@example.invalid")
            .unwrap()
            .unwrap()
            .1;
        (
            ai,
            format!(
                "__Host-webts={}",
                app.db.create_session(ai, false, &hash).unwrap()
            ),
        )
    };
    assert_eq!(
        call(&app, "/preferences", None, &second, "", ORIGIN)
            .await
            .1["preferences"]["burnSeconds"],
        600
    );
    assert_eq!(
        router(app.clone())
            .oneshot(patch(&second, change(ai, json!({"theme":"light"}))))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let values = call(&app, "/preferences", None, &a, "", ORIGIN).await.1["preferences"].clone();
    assert_eq!(values["burn"], true);
    assert_eq!(values["theme"], "light");
    assert_eq!(
        call(&app, "/preferences", None, &b, "", ORIGIN).await.1["preferences"],
        json!({})
    );
    for invalid in [
        change(bi, json!({"burn":false})),
        change(ai, json!({"password":"secret"})),
        change(ai, json!({"audio":{"input":"private-device-id"}})),
        change(ai, json!({"burnSeconds":604801})),
        change(ai, json!({"__proto__":{"polluted":true}})),
    ] {
        assert_eq!(
            router(app.clone())
                .oneshot(patch(&a, invalid))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        router(app.clone())
            .oneshot(patch(
                &second,
                json!({"account":ai,"patch":{"burn":false},"initialize":true})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/preferences", None, &a, "", ORIGIN).await.1["preferences"]["burn"],
        true,
        "other browser initialization never overwrites saved preferences"
    );
    call(&app, "/auth/logout", Some(json!({})), &a, "", ORIGIN).await;
    assert_eq!(
        router(app.clone())
            .oneshot(patch(&a, change(ai, json!({"burn":false}))))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn ordinary_chat_needs_no_peer_consent_or_private_identity_and_cannot_read_e2ee() {
    let (_root, app) = setup();
    let (ai, a) = user(&app, "ordinary-a@example.invalid");
    let (bi, b) = user(&app, "ordinary-b@example.invalid");
    let (_, outsider) = user(&app, "ordinary-outsider@example.invalid");
    for cookie in [&a, &b] {
        assert_eq!(
            call(&app, "/friends/me", None, cookie, "", ORIGIN).await.0,
            StatusCode::OK
        );
    }
    app.db
        .connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO friendships VALUES(?,?,?,1,0,0)",
            rusqlite::params![ai, bi, ai],
        )
        .unwrap();
    let rows = call(&app, "/friends", None, &a, "", ORIGIN).await.1;
    assert_eq!(rows["friends"][0]["allow_server"], true);
    assert_eq!(rows["friends"][0]["mode_epoch"], 0);
    assert_eq!(rows["friends"][0]["public_key"], "");
    let mid = "f041b157-319f-4bf3-970e-7a04e30bd070";
    assert_eq!(call(&app,"/friends/messages",Some(json!({"peer":bi,"id":mid,"ciphertext":readable_packet(ai,bi,mid,0),"mode":"server","epoch":0})),&a,"",ORIGIN).await.0,StatusCode::SERVICE_UNAVAILABLE,"passes identity and content checks; storage remains mandatory");
    {
        let db = app.db.connection.lock().unwrap();
        for (id, mode) in [("normal-history", "server"), ("e2ee-history", "e2ee")] {
            db.execute("INSERT INTO friend_messages(id,sender,recipient,object_key,content_hash,created_at,expires,ready,mode,mode_epoch,burn,burn_seconds) VALUES(?,?,?,?,?,0,?,1,?,2,1,60)",rusqlite::params![id,ai,bi,id,"hash",web_ts::db::now()+3600,mode]).unwrap();
        }
    }
    let rows = call(
        &app,
        &format!("/friends/{ai}/messages"),
        None,
        &b,
        "",
        ORIGIN,
    )
    .await;
    assert_eq!(rows.0, StatusCode::OK);
    assert_eq!(rows.1["messages"].as_array().unwrap().len(), 1);
    assert_eq!(rows.1["messages"][0]["id"], "normal-history");
    assert_eq!(
        call(&app, "/friends/messages/e2ee-history", None, &b, "", ORIGIN)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let read = call(
        &app,
        "/friends/messages/normal-history/read",
        Some(json!({})),
        &b,
        "",
        ORIGIN,
    )
    .await;
    assert_eq!(read.0, StatusCode::OK);
    assert!(read.1["expires"].as_i64().unwrap() <= web_ts::db::now() + 60);
    assert_eq!(
        call(
            &app,
            "/friends/messages/normal-history/read",
            Some(json!({})),
            &outsider,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "/friends/messages/e2ee-history/read",
            Some(json!({})),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{ai}/encryption"),
            Some(json!({"server_decrypt":false})),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(call(&app,"/friends/messages",Some(json!({"peer":bi,"id":mid,"ciphertext":readable_packet(ai,bi,mid,0),"mode":"server","epoch":0})),&a,"",ORIGIN).await.0,StatusCode::CONFLICT);
    assert_eq!(call(&app,"/friends/messages",Some(json!({"peer":bi,"id":mid,"ciphertext":readable_packet(ai,bi,mid,1),"mode":"server","epoch":1})),&a,"",ORIGIN).await.0,StatusCode::SERVICE_UNAVAILABLE);
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("DELETE FROM sessions WHERE user_id=?", [bi])
        .unwrap();
    assert_eq!(call(&app,"/friends/messages",Some(json!({"peer":bi,"id":mid,"ciphertext":readable_packet(ai,bi,mid,1),"mode":"server","epoch":1})),&a,"",ORIGIN).await.0,StatusCode::SERVICE_UNAVAILABLE,"recipient offline does not block delivery; enabled bucket still required");
    let protected = call(
        &app,
        &format!("/friends/{bi}/encryption"),
        Some(json!({"server_decrypt":false})),
        &a,
        "",
        ORIGIN,
    )
    .await;
    assert_eq!(protected.1["epoch"], 2);
    assert_eq!(call(&app,"/friends/messages",Some(json!({"peer":bi,"id":mid,"ciphertext":readable_packet(ai,bi,mid,2),"mode":"server","epoch":2})),&a,"",ORIGIN).await.0,StatusCode::CONFLICT,"an explicit E2EE sender choice cannot silently fall back to ordinary mode");
    let ordinary = call(
        &app,
        &format!("/friends/{bi}/encryption"),
        Some(json!({"server_decrypt":true})),
        &a,
        "",
        ORIGIN,
    )
    .await;
    assert_eq!(ordinary.1["mode"], "server");
    assert_eq!(ordinary.1["epoch"], 3);
    assert_eq!(call(&app,"/friends/messages",Some(json!({"peer":bi,"id":mid,"ciphertext":readable_packet(ai,bi,mid,3),"mode":"server","epoch":3})),&a,"",ORIGIN).await.0,StatusCode::SERVICE_UNAVAILABLE,"no acknowledgement field or peer consent required");
}

#[tokio::test]
async fn presence_is_friends_only_hidden_on_request_and_revoked_with_session() {
    let (_root, app) = setup();
    let (ai, a) = user(&app, "presence-a@example.invalid");
    let (bi, b) = user(&app, "presence-b@example.invalid");
    let (_, outsider) = user(&app, "presence-outsider@example.invalid");
    for cookie in [&a, &b] {
        call(&app, "/friends/me", None, cookie, "", ORIGIN).await;
    }
    app.db
        .connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO friendships VALUES(?,?,?,0,0,0)",
            rusqlite::params![ai, bi, ai],
        )
        .unwrap();
    assert!(
        call(&app, "/friends", None, &a, "", ORIGIN).await.1["friends"][0]
            .get("online")
            .is_none()
    );
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE friendships SET status=1", [])
        .unwrap();
    assert_eq!(
        call(&app, "/friends", None, &a, "", ORIGIN).await.1["friends"][0]["online"],
        true
    );
    assert_eq!(
        call(&app, "/friends", None, &outsider, "", ORIGIN).await.1["friends"],
        json!([])
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{bi}/join"),
            Some(json!({"connection":"arbitrary"})),
            &outsider,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/friends/presence",
            Some(json!({"share":false})),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/friends", None, &a, "", ORIGIN).await.1["friends"][0]["online"],
        false
    );
    call(
        &app,
        "/friends/presence",
        Some(json!({"share":true})),
        &b,
        "",
        ORIGIN,
    )
    .await;
    assert_eq!(
        call(&app, "/auth/logout", Some(json!({})), &b, "", ORIGIN)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/friends", None, &a, "", ORIGIN).await.1["friends"][0]["online"],
        false
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{bi}/join"),
            Some(json!({"connection":"arbitrary"})),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn account_friends_share_updated_profiles_without_exposing_email_or_needing_chat_keys() {
    let (_root, app) = setup();
    let (alice, ac) = user(&app, "community-alice@example.invalid");
    let (bob, bc) = user(&app, "community-bob@example.invalid");
    let (_, stranger) = user(&app, "community-outsider@example.invalid");
    let code = call(&app, "/friends/me", None, &bc, "", ORIGIN).await.1["code"].clone();
    assert_eq!(
        call(
            &app,
            "/friends/request",
            Some(json!({"code":code})),
            &ac,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK,
        "friendship must not require initializing encryption first"
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{alice}"),
            Some(json!({"action":"accept"})),
            &bc,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    use base64::Engine;
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
        16,
        16,
        image::Rgba([30, 70, 210, 255]),
    ))
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let avatar = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
    assert_eq!(call(&app,"/profile",Some(json!({"display_name":"社区昵称","about":"hello community","avatar":avatar,"sync_avatar":false,"sync_about":false})),&bc,"",ORIGIN).await.0,StatusCode::OK);
    let listing = call(&app, "/friends", None, &ac, "", ORIGIN).await.1;
    let friend = &listing["friends"][0];
    assert_eq!(friend["name"], "社区昵称");
    assert_eq!(friend["about"], "hello community");
    assert_eq!(friend["avatar_hash"].as_str().unwrap().len(), 64);
    assert!(
        !listing
            .to_string()
            .contains("community-bob@example.invalid")
    );
    let request = |cookie: &str| {
        Request::builder()
            .uri(format!("/api/friends/{bob}/avatar"))
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap()
    };
    let response = router(app.clone()).oneshot(request(&ac)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    let etag = response.headers()["etag"].clone();
    assert!(
        response.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    let bytes = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    assert_eq!(
        image::guess_format(&bytes).unwrap(),
        image::ImageFormat::Png
    );
    let cached = Request::builder()
        .uri(format!("/api/friends/{bob}/avatar"))
        .header("cookie", &ac)
        .header("if-none-match", etag.clone())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router(app.clone()).oneshot(cached).await.unwrap().status(),
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        router(app.clone())
            .oneshot(request(&stranger))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{bob}"),
            Some(json!({"action":"block"})),
            &ac,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    let denied_cached = Request::builder()
        .uri(format!("/api/friends/{bob}/avatar"))
        .header("cookie", &ac)
        .header("if-none-match", etag)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router(app.clone())
            .oneshot(denied_cached)
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn storage_region_correction_keeps_existing_objects_and_blocks_real_migration() {
    let (_root, app) = setup();
    let (owner, cookie) = user(&app, "storage-admin@example.invalid");
    let (peer, _) = user(&app, "storage-peer@example.invalid");
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE users SET is_admin=1 WHERE id=?", [owner])
        .unwrap();
    let old = web_ts::storage::Storage {
        enabled: true,
        endpoint: "https://cn-sy1.rains3.com".into(),
        bucket: "fixture-community".into(),
        region: "321".into(),
        access_key: "synthetic-access".into(),
        secret_key: "synthetic secret only".into(),
        retention_days: 7,
    };
    app.runtime.write().unwrap().settings.storage = old.clone();
    app.db.connection.lock().unwrap().execute("INSERT INTO friend_messages(id,sender,recipient,object_key,content_hash,created_at,expires,ready) VALUES('region-fixture',?,?,?,'synthetic-hash',0,?,0)",rusqlite::params![owner,peer,"a".repeat(64),web_ts::db::now()+3600]).unwrap();
    let mut corrected = serde_json::to_value(&old).unwrap();
    corrected["region"] = json!("");
    corrected["endpoint"] = json!("https://cn-sy1.rains3.com/");
    corrected["access_key"] = json!("");
    corrected["secret_key"] = json!("");
    let result = call(
        &app,
        "/admin/storage",
        Some(json!({"password":PASSWORD,"storage":corrected})),
        &cookie,
        "",
        ORIGIN,
    )
    .await;
    assert_eq!(
        result.0,
        StatusCode::OK,
        "same bucket signing correction must not be treated as a migration: {}",
        result.1
    );
    assert_eq!(
        app.runtime.read().unwrap().settings.storage.region,
        "us-east-1"
    );
    assert_eq!(
        app.db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM friend_messages WHERE id='region-fixture'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    let mut moved = serde_json::to_value(&app.runtime.read().unwrap().settings.storage).unwrap();
    moved["bucket"] = json!("different-community");
    assert_eq!(
        call(
            &app,
            "/admin/storage",
            Some(json!({"password":PASSWORD,"storage":moved})),
            &cookie,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let mut disabled = serde_json::to_value(&app.runtime.read().unwrap().settings.storage).unwrap();
    disabled["enabled"] = json!(false);
    assert_eq!(
        call(
            &app,
            "/admin/storage",
            Some(json!({"password":PASSWORD,"storage":disabled})),
            &cookie,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "/admin/storage/test",
            Some(json!({"password":PASSWORD})),
            "",
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, regular) = user(&app, "storage-outsider@example.invalid");
    assert_eq!(
        call(
            &app,
            "/admin/storage/test",
            Some(json!({"password":PASSWORD})),
            &regular,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/admin/storage/test",
            Some(json!({"password":"wrong"})),
            &cookie,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "/admin/storage/test",
            Some(json!({"password":PASSWORD})),
            &cookie,
            "",
            "https://outsider.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // Even before a message index is reserved, an active object operation prevents migration.
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("DELETE FROM friend_messages", [])
        .unwrap();
    let permit = app.friend_objects.clone().try_acquire_owned().unwrap();
    let mut moved = serde_json::to_value(&app.runtime.read().unwrap().settings.storage).unwrap();
    moved["bucket"] = json!("another-community");
    assert_eq!(
        call(
            &app,
            "/admin/storage",
            Some(json!({"password":PASSWORD,"storage":moved})),
            &cookie,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    drop(permit);
}
fn setup() -> (tempfile::TempDir, Arc<App>) {
    let root = tempfile::tempdir().unwrap();
    let key = root.path().join("key");
    std::fs::write(&key, hex::encode([29; 32])).unwrap();
    let template: Config = toml::from_str(include_str!("../../config.example.toml")).unwrap();
    let app = App::new(Config {
        database: root.path().join("db").to_string_lossy().into_owned(),
        master_key_file: key.to_string_lossy().into_owned(),
        public_url: ORIGIN.into(),
        ..template
    })
    .unwrap();
    (root, app)
}
fn user(app: &App, email: &str) -> (i64, String) {
    let hash = password::hash(PASSWORD).unwrap();
    let (id, _, t) = app.db.register(email, &hash).unwrap();
    app.db.consume_email_token(&t, "verify", None).unwrap();
    (
        id,
        format!(
            "__Host-webts={}",
            app.db.create_session(id, false, &hash).unwrap()
        ),
    )
}
async fn call(
    app: &Arc<App>,
    path: &str,
    body: Option<Value>,
    cookie: &str,
    lease: &str,
    origin: &str,
) -> (StatusCode, Value) {
    let r = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(format!("/api{path}"))
        .header("origin", origin)
        .header("cookie", cookie)
        .header("x-webts-chat-device", lease)
        .header("content-type", "application/json")
        .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
        .unwrap();
    let r = router(app.clone()).oneshot(r).await.unwrap();
    let status = r.status();
    let b = to_bytes(r.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&b).unwrap_or(Value::Null))
}
async fn register(app: &Arc<App>, cookie: &str, engine: &webts_crypto::Engine) -> String {
    assert_eq!(
        call(
            app,
            "/friends/key",
            Some(json!({"password":PASSWORD,"public_key":engine.identity()})),
            cookie,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    call(app, "/friends/me", None, cookie, "", ORIGIN).await.1["code"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn activation(e: &mut webts_crypto::Engine, device: &str, lease: &str) -> Value {
    let mut v: Value = serde_json::from_str(&e.prekeys().unwrap()).unwrap();
    v["password"] = json!(PASSWORD);
    v["device"] = json!(device);
    v["lease"] = json!(lease);
    v
}
fn readable_packet(sender: i64, recipient: i64, id: &str, epoch: i64) -> String {
    use aes_gcm::{
        Aes256Gcm, KeyInit, Nonce,
        aead::{Aead, Payload},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut key = [0; 32];
    let mut iv = [0; 12];
    getrandom::fill(&mut key).unwrap();
    getrandom::fill(&mut iv).unwrap();
    let value = json!({"version":1,"site":ORIGIN,"id":id,"sender":sender,"recipient":recipient,"expires":web_ts::db::now()+3600,"text":"synthetic readable fixture","burn":false});
    let aad = format!("webts-server-content-v1:{ORIGIN}:{id}:{sender}:{recipient}:false:{epoch}");
    let content = Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .encrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: value.to_string().as_bytes(),
                aad: aad.as_bytes(),
            },
        )
        .unwrap();
    json!({"version":1,"mode":"server","epoch":epoch,"key":STANDARD.encode(key),"iv":STANDARD.encode(iv),"content":STANDARD.encode(content)}).to_string()
}
#[tokio::test]
async fn friendships_require_acceptance_and_keys_are_immutable_and_signed() {
    let (_root, app) = setup();
    let (_, a) = user(&app, "alice@example.invalid");
    let (_, b) = user(&app, "bob@example.invalid");
    let (_, other) = user(&app, "other@example.invalid");
    let mut ae = webts_crypto::Engine::new();
    let mut be = webts_crypto::Engine::new();
    let code = register(&app, &b, &be).await;
    register(&app, &a, &ae).await;
    assert_eq!(
        call(&app, "/friends/me", None, "", "", ORIGIN).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "/friends/request",
            Some(json!({"code":code})),
            &a,
            "",
            "https://attacker.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (ai, bi) = {
        let db = app.db.connection.lock().unwrap();
        let ids: Vec<i64> = db
            .prepare("SELECT id FROM users ORDER BY id LIMIT 2")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        (ids[0], ids[1])
    };
    assert_eq!(
        call(
            &app,
            "/friends/request",
            Some(json!({"code":code})),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{bi}"),
            Some(json!({"action":"accept"})),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{ai}"),
            Some(json!({"action":"accept"})),
            &other,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{ai}"),
            Some(json!({"action":"accept"})),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "/friends/key",
            Some(json!({"password":PASSWORD,"public_key":be.identity()})),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let lease = "a".repeat(64);
    let device = "c041b157-319f-4bf3-970e-7a04e30bd070";
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(activation(&mut ae, device, &lease)),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut bad = activation(&mut be, device, &"b".repeat(64));
    bad["signature"] = json!("A".repeat(86));
    assert_eq!(
        call(&app, "/friends/device", Some(bad), &b, "", ORIGIN)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(activation(&mut be, device, &"b".repeat(64))),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    let one = call(
        &app,
        &format!("/friends/{bi}/prekey"),
        Some(json!({})),
        &a,
        &lease,
        ORIGIN,
    )
    .await;
    let two = call(
        &app,
        &format!("/friends/{bi}/prekey"),
        Some(json!({})),
        &a,
        &lease,
        ORIGIN,
    )
    .await;
    assert_eq!(one.0, StatusCode::OK);
    assert_ne!(one.1["key"], two.1["key"]);
    assert_eq!(
        call(
            &app,
            &format!("/friends/{bi}/messages"),
            None,
            &a,
            &"c".repeat(64),
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    let replacement = "d".repeat(64);
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(activation(&mut ae, device, &replacement)),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{bi}/messages"),
            None,
            &a,
            &lease,
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{bi}"),
            Some(json!({"action":"block"})),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{ai}"),
            Some(json!({"action":"unblock"})),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{bi}/prekey"),
            Some(json!({})),
            &a,
            &replacement,
            ORIGIN
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "/friends", None, &b, "", ORIGIN).await.1["friends"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        call(&app, "/friends", None, &a, "", ORIGIN).await.1["friends"][0]["status"],
        2
    );
}
#[tokio::test]
async fn expired_objects_burn_ack_and_account_isolation_are_enforced_before_bucket_access() {
    let (_root, app) = setup();
    let (ai, a) = user(&app, "alice@example.invalid");
    let (bi, b) = user(&app, "bob@example.invalid");
    let (_, other) = user(&app, "other@example.invalid");
    let ae = webts_crypto::Engine::new();
    let mut be = webts_crypto::Engine::new();
    register(&app, &a, &ae).await;
    register(&app, &b, &be).await;
    let lease = "b".repeat(64);
    let device = "c041b157-319f-4bf3-970e-7a04e30bd070";
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(activation(&mut be, device, &lease)),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    let mid = "b041b157-319f-4bf3-970e-7a04e30bd070";
    let old = "a041b157-319f-4bf3-970e-7a04e30bd070";
    {
        let db = app.db.connection.lock().unwrap();
        db.execute(
            "INSERT INTO friendships VALUES(?,?,?,1,0,0)",
            rusqlite::params![ai, bi, ai],
        )
        .unwrap();
        for (id, expires, burn) in [(mid, web_ts::db::now() + 3600, true), (old, 1, false)] {
            db.execute("INSERT INTO friend_messages(id,sender,recipient,object_key,content_hash,created_at,expires,ready,burn) VALUES(?,?,?,?,?,0,?,1,?)",rusqlite::params![id,ai,bi,id,"hash",expires,burn]).unwrap();
        }
    }
    assert_eq!(
        call(
            &app,
            &format!("/friends/messages/{old}"),
            None,
            &b,
            &lease,
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "/friends", None, &b, "", ORIGIN).await.1["unread"],
        1
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/messages/{mid}/read"),
            Some(json!({})),
            &other,
            &lease,
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/messages/{mid}/read"),
            Some(json!({})),
            &b,
            &lease,
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/messages/{mid}/read"),
            Some(json!({})),
            &b,
            &lease,
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/messages/{mid}"),
            None,
            &b,
            &lease,
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let queued = "d041b157-319f-4bf3-970e-7a04e30bd070";
    app.db.connection.lock().unwrap().execute("INSERT INTO friend_messages(id,sender,recipient,object_key,content_hash,created_at,expires,ready,burn) VALUES(?,?,?,?,?,0,?,1,0)",rusqlite::params![queued,ai,bi,queued,"hash",web_ts::db::now()+3600]).unwrap();
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(activation(&mut be, device, &lease)),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{ai}/messages"),
            None,
            &b,
            &lease,
            ORIGIN
        )
        .await
        .1["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let new_lease = "e".repeat(64);
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(activation(
                &mut be,
                "e041b157-319f-4bf3-970e-7a04e30bd070",
                &new_lease
            )),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/{ai}/messages"),
            None,
            &b,
            &new_lease,
            ORIGIN
        )
        .await
        .1["messages"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        call(
            &app,
            &format!("/friends/messages/{queued}"),
            None,
            &b,
            &new_lease,
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE users SET banned_until=-1 WHERE id=?", [bi])
        .unwrap();
    assert_eq!(
        call(&app, "/friends", None, &b, "", ORIGIN).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn chosen_burn_timer_starts_once_on_recipient_read_and_never_extends_retention() {
    let (_root, app) = setup();
    let (ai, a) = user(&app, "timer-alice@example.invalid");
    let (bi, b) = user(&app, "timer-bob@example.invalid");
    let ae = webts_crypto::Engine::new();
    let mut be = webts_crypto::Engine::new();
    register(&app, &a, &ae).await;
    register(&app, &b, &be).await;
    for seconds in [-1, 0, 59, 61, 604801] {
        let ciphertext=json!({"version":1,"device":"b041b157-319f-4bf3-970e-7a04e30bd070","packet":"opaque","iv":"AQEBAQEBAQEBAQEB","content":"AAAA"}).to_string();
        assert_eq!(call(&app,"/friends/messages",Some(json!({"peer":bi,"id":"c041b157-319f-4bf3-970e-7a04e30bd070","ciphertext":ciphertext,"burn":true,"burn_seconds":seconds})),&a,"",ORIGIN).await.0,StatusCode::BAD_REQUEST);
    }
    let lease = "b".repeat(64);
    let device = "c041b157-319f-4bf3-970e-7a04e30bd070";
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(activation(&mut be, device, &lease)),
            &b,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    {
        let db = app.db.connection.lock().unwrap();
        db.execute(
            "INSERT INTO friendships VALUES(?,?,?,1,0,0)",
            rusqlite::params![ai, bi, ai],
        )
        .unwrap();
    }
    for seconds in [60, 600, 1800, 3600, 43200, 86400, 172800, 345600, 604800] {
        let id = format!("{seconds:08x}-319f-4bf3-970e-7a04e30bd070");
        let cap = web_ts::db::now() + 4000;
        app.db.connection.lock().unwrap().execute("INSERT INTO friend_messages(id,sender,recipient,object_key,content_hash,created_at,expires,ready,burn,burn_seconds) VALUES(?,?,?,?,?,0,?,1,1,?)",rusqlite::params![id,ai,bi,id,"hash",cap,seconds]).unwrap();
        let path = format!("/friends/messages/{id}/read");
        let (status, ack) = call(&app, &path, Some(json!({})), &b, &lease, ORIGIN).await;
        assert_eq!(status, StatusCode::OK);
        let read = ack["read_at"].as_i64().unwrap();
        assert_eq!(ack["expires"], cap.min(read + seconds));
        let again = call(&app, &path, Some(json!({})), &b, &lease, ORIGIN)
            .await
            .1;
        assert_eq!(again["read_at"], ack["read_at"]);
        assert_eq!(again["expires"], ack["expires"]);
        let list = call(
            &app,
            &format!("/friends/{ai}/messages"),
            None,
            &b,
            &lease,
            ORIGIN,
        )
        .await
        .1;
        let row = list["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap();
        assert_eq!(row["burn_seconds"], seconds);
        app.db
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE friend_messages SET expires=1 WHERE id=?", [&id])
            .unwrap();
        assert_eq!(
            call(
                &app,
                &format!("/friends/messages/{id}"),
                None,
                &b,
                &lease,
                ORIGIN
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
}
#[tokio::test]
async fn removing_requests_does_not_reset_account_rate_limits() {
    let (_root, app) = setup();
    let (ai, a) = user(&app, "alice@example.invalid");
    let (bi, b) = user(&app, "bob@example.invalid");
    register(&app, &a, &webts_crypto::Engine::new()).await;
    let code = register(&app, &b, &webts_crypto::Engine::new()).await;
    for _ in 0..5 {
        assert_eq!(
            call(
                &app,
                "/friends/request",
                Some(json!({"code":code})),
                &a,
                "",
                ORIGIN
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            call(
                &app,
                &format!("/friends/{bi}"),
                Some(json!({"action":"decline"})),
                &a,
                "",
                ORIGIN
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        call(
            &app,
            "/friends/request",
            Some(json!({"code":code})),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let limits: i64 = app
        .db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT count FROM friend_request_limits WHERE user_id=?",
            [ai],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(limits, 5);
}

#[tokio::test]
async fn sender_encryption_choice_is_independent_and_invalidates_inflight_uploads() {
    let (_root, app) = setup();
    let (ai, a) = user(&app, "alice@example.invalid");
    let (bi, b) = user(&app, "bob@example.invalid");
    let mut ae = webts_crypto::Engine::new();
    let mut be = webts_crypto::Engine::new();
    register(&app, &a, &ae).await;
    register(&app, &b, &be).await;
    let la = "a".repeat(64);
    let lb = "b".repeat(64);
    for (cookie, engine, lease, device) in [
        (&a, &mut ae, &la, "a041b157-319f-4bf3-970e-7a04e30bd070"),
        (&b, &mut be, &lb, "b041b157-319f-4bf3-970e-7a04e30bd070"),
    ] {
        assert_eq!(
            call(
                &app,
                "/friends/device",
                Some(activation(engine, device, lease)),
                cookie,
                "",
                ORIGIN
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let pa = format!("/friends/{bi}/encryption");
    let pb = format!("/friends/{ai}/encryption");
    let consent = json!({"server_decrypt":true,"acknowledge":true,"password":PASSWORD});
    assert_eq!(
        call(&app, &pa, Some(consent.clone()), &a, &la, ORIGIN)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    {
        let db = app.db.connection.lock().unwrap();
        db.execute(
            "INSERT INTO friendships VALUES(?,?,?,1,0,0)",
            rusqlite::params![ai, bi, ai],
        )
        .unwrap();
        // Preserve an explicitly chosen legacy E2EE conversation.
        db.execute(
            "INSERT INTO friend_modes(lo,hi,allow_lo,allow_hi) VALUES(?,?,0,0)",
            rusqlite::params![ai, bi],
        )
        .unwrap();
        for (id, ready) in [("pending-mode", 0), ("historical-e2ee", 1)] {
            db.execute("INSERT INTO friend_messages(id,sender,recipient,object_key,content_hash,created_at,expires,ready) VALUES(?,?,?,?,?,0,?,?)",rusqlite::params![id,ai,bi,id,"hash",web_ts::db::now()+3600,ready]).unwrap();
        }
    }
    assert_eq!(
        call(
            &app,
            &pa,
            Some(consent.clone()),
            &a,
            &la,
            "https://attacker.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            &pa,
            Some(json!({"server_decrypt":true,"password":PASSWORD})),
            &a,
            &la,
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &pa,
            Some(json!({"server_decrypt":true,"acknowledge":true,"password":"wrong password"})),
            &a,
            &la,
            ORIGIN
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, &pa, Some(consent.clone()), &a, &lb, ORIGIN)
            .await
            .0,
        StatusCode::OK
    );
    let first = call(&app, &pa, Some(consent.clone()), &a, &la, ORIGIN).await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.1["mode"], "server");
    assert_eq!(first.1["epoch"], 1);
    assert_eq!(
        call(&app, &pa, Some(consent.clone()), &a, &la, ORIGIN)
            .await
            .1["epoch"],
        1
    );
    let second = call(&app, &pb, Some(consent.clone()), &b, &lb, ORIGIN).await;
    assert_eq!(second.0, StatusCode::OK);
    assert_eq!(second.1["mode"], "server");
    assert_eq!(second.1["epoch"], 2);
    let rows = call(&app, "/friends", None, &b, "", ORIGIN).await.1;
    assert_eq!(rows["friends"][0]["allow_server"], true);
    assert_eq!(rows["friends"][0]["peer_allow_server"], true);
    assert_eq!(rows["friends"][0]["mode_epoch"], 2);
    let mid = "c041b157-319f-4bf3-970e-7a04e30bd070";
    let valid = readable_packet(ai, bi, mid, 2);
    assert_eq!(
        call(
            &app,
            "/friends/messages",
            Some(json!({"peer":bi,"id":mid,"ciphertext":valid,"mode":"server","epoch":2})),
            &a,
            &la,
            ORIGIN
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let swapped = readable_packet(bi, ai, mid, 2);
    assert_eq!(
        call(
            &app,
            "/friends/messages",
            Some(json!({"peer":bi,"id":mid,"ciphertext":swapped,"mode":"server","epoch":2})),
            &a,
            &la,
            ORIGIN
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let encrypted=json!({"version":1,"device":"b041b157-319f-4bf3-970e-7a04e30bd070","packet":"opaque","iv":"AQEBAQEBAQEBAQEB","content":"AAAA"}).to_string();
    assert_eq!(
        call(
            &app,
            "/friends/messages",
            Some(json!({"peer":bi,"id":mid,"ciphertext":encrypted,"mode":"e2ee","epoch":2})),
            &a,
            &la,
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "/friends/messages",
            Some(json!({"peer":bi,"id":mid,"ciphertext":"{}","mode":"server","epoch":1})),
            &a,
            &la,
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "/friends/messages",
            Some(json!({"peer":bi,"id":mid,"ciphertext":"{}","mode":"server","epoch":2})),
            &a,
            &la,
            ORIGIN
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let historical = call(
        &app,
        &format!("/friends/{ai}/messages"),
        None,
        &b,
        &lb,
        ORIGIN,
    )
    .await
    .1;
    assert_eq!(historical["messages"].as_array().unwrap().len(), 1);
    assert_eq!(historical["messages"][0]["mode"], "e2ee");
    assert_eq!(historical["messages"][0]["epoch"], 0);
    {
        let db = app.db.connection.lock().unwrap();
        let pending: i64 = db
            .query_row(
                "SELECT expires FROM friend_messages WHERE id='pending-mode'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(pending <= web_ts::db::now());
    }
    let restored = call(
        &app,
        &pb,
        Some(json!({"server_decrypt":false})),
        &b,
        &lb,
        ORIGIN,
    )
    .await;
    assert_eq!(restored.0, StatusCode::OK);
    assert_eq!(restored.1["mode"], "e2ee");
    assert_eq!(restored.1["epoch"], 3);
    assert_eq!(
        call(&app, &pb, Some(consent), &b, &lb, ORIGIN).await.1["epoch"],
        4
    );
    assert_eq!(
        call(
            &app,
            "/friends/messages",
            Some(json!({"peer":bi,"id":mid,"ciphertext":"{}","mode":"server","epoch":2})),
            &a,
            &la,
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE users SET banned_until=-1 WHERE id=?", [ai])
        .unwrap();
    assert_eq!(
        call(
            &app,
            &pa,
            Some(json!({"server_decrypt":false})),
            &a,
            &la,
            ORIGIN
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE users SET banned_until=0 WHERE id=?", [ai])
        .unwrap();
    assert_eq!(
        call(
            &app,
            &format!("/friends/{bi}"),
            Some(json!({"action":"remove"})),
            &a,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        app.db
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM friend_modes", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn automatic_activation_requires_the_current_lease_and_cannot_take_over_another_device() {
    let (_root, app) = setup();
    let (_, cookie) = user(&app, "auto@example.invalid");
    let mut engine = webts_crypto::Engine::new();
    register(&app, &cookie, &engine).await;
    let device = "a041b157-319f-4bf3-970e-7a04e30bd070";
    let first = "a".repeat(64);
    let second = "b".repeat(64);
    let mut body = activation(&mut engine, device, &first);
    body["password"] = json!("");
    assert_eq!(
        call(&app, "/friends/device", Some(body), &cookie, &first, ORIGIN)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(activation(&mut engine, device, &first)),
            &cookie,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut rotate = activation(&mut engine, device, &second);
    rotate["password"] = json!("");
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(rotate.clone()),
            &cookie,
            &first,
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(rotate.clone()),
            &cookie,
            &first,
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(rotate),
            &cookie,
            &second,
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut takeover = activation(
        &mut engine,
        "c041b157-319f-4bf3-970e-7a04e30bd070",
        &"c".repeat(64),
    );
    takeover["password"] = json!("");
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(takeover),
            &cookie,
            &second,
            ORIGIN
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("DELETE FROM sessions", [])
        .unwrap();
    let mut revoked = activation(&mut engine, device, &second);
    revoked["password"] = json!("");
    assert_eq!(
        call(
            &app,
            "/friends/device",
            Some(revoked),
            &cookie,
            &second,
            ORIGIN
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[test]
fn migration_preserves_old_opaque_messages_as_e2ee_and_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("old.sqlite");
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE users(id INTEGER PRIMARY KEY,email TEXT UNIQUE NOT NULL,password_hash TEXT NOT NULL,verified INTEGER NOT NULL DEFAULT 0);INSERT INTO users VALUES(1,'a@example.invalid','hash',1),(2,'b@example.invalid','hash',1);CREATE TABLE friend_messages(seq INTEGER PRIMARY KEY,id TEXT UNIQUE NOT NULL,sender INTEGER NOT NULL,recipient INTEGER NOT NULL,object_key TEXT UNIQUE NOT NULL,content_hash TEXT NOT NULL,created_at INTEGER NOT NULL,expires INTEGER NOT NULL,ready INTEGER NOT NULL DEFAULT 0,read_at INTEGER,burn INTEGER NOT NULL DEFAULT 0);INSERT INTO friend_messages VALUES(7,'old-message',1,2,'opaque-object','opaque-hash',123,456,1,NULL,0);").unwrap();
    }
    for _ in 0..2 {
        let db = web_ts::db::Db::open(path.to_str().unwrap()).unwrap();
        let conn = db.connection.lock().unwrap();
        let row: (i64, String, String, String, i64) = conn
            .query_row(
                "SELECT seq,object_key,content_hash,mode,mode_epoch FROM friend_messages",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                7,
                "opaque-object".into(),
                "opaque-hash".into(),
                "e2ee".into(),
                0
            )
        );
    }
}
