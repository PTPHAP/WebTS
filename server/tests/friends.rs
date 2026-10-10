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
        StatusCode::CONFLICT
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
        StatusCode::CONFLICT
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
        StatusCode::CONFLICT
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
