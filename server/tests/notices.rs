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
    site::policy_version,
};
const ORIGIN: &str = "https://letters.example";
const PASSWORD: &str = "synthetic notification password only";
fn setup() -> (tempfile::TempDir, Arc<App>) {
    let root = tempfile::tempdir().unwrap();
    let key = root.path().join("key");
    std::fs::write(&key, hex::encode([19; 32])).unwrap();
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
fn user(app: &App, email: &str, verified: bool) -> (i64, String) {
    let hash = password::hash(PASSWORD).unwrap();
    let (id, _, token) = app.db.register(email, &hash).unwrap();
    if verified {
        app.db.consume_email_token(&token, "verify", None).unwrap();
    }
    let cookie = if verified {
        format!(
            "__Host-webts={}",
            app.db.create_session(id, false, &hash).unwrap()
        )
    } else {
        String::new()
    };
    (id, cookie)
}
async fn call(
    app: &Arc<App>,
    path: &str,
    body: Option<Value>,
    cookie: &str,
    origin: &str,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let request = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(format!("/api{path}"))
        .header("content-type", "application/json")
        .header("origin", origin)
        .header("cookie", cookie)
        .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
        .unwrap();
    let response = router(app.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn draft(ids: Vec<i64>) -> Value {
    json!({"title":"测试更新","body":"# 更新\n\n**新功能** [查看](https://example.com)\n<script>steal()</script>","format":"markdown","audience":"selected","recipients":ids,"email":false,"password":PASSWORD,"request_id":"2e3f05f0-d4d6-4d48-9176-919d67ba5698"})
}
#[tokio::test]
async fn inbox_publication_is_admin_only_isolated_idempotent_and_revocable() {
    let (_root, app) = setup();
    let (_, admin) = user(&app, "admin@example.invalid", true);
    app.db.grant_admin("admin@example.invalid").unwrap();
    let (first, one) = user(&app, "one@example.invalid", true);
    let (_, two) = user(&app, "two@example.invalid", true);
    let (pending, _) = user(&app, "pending@example.invalid", false);
    let (banned, _) = user(&app, "banned@example.invalid", true);
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE users SET banned_until=-1 WHERE id=?", [banned])
        .unwrap();
    let d = draft(vec![first, pending, banned]);
    assert_eq!(
        call(&app, "/notices", None, "", ORIGIN).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "/admin/notices/preview",
            Some(d.clone()),
            &one,
            ORIGIN
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/admin/notices",
            Some(d.clone()),
            &admin,
            "https://evil.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut wrong = d.clone();
    wrong["password"] = json!("wrong");
    assert_eq!(
        call(&app, "/admin/notices", Some(wrong), &admin, ORIGIN)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let preview = call(
        &app,
        "/admin/notices/preview",
        Some(d.clone()),
        &admin,
        ORIGIN,
    )
    .await;
    assert_eq!(preview.0, StatusCode::OK);
    assert_eq!(preview.2["recipients"], 1);
    assert_eq!(preview.2["email_recipients"], 0);
    assert!(!preview.2["html"].as_str().unwrap().contains("steal"));
    let published = call(&app, "/admin/notices", Some(d.clone()), &admin, ORIGIN).await;
    assert_eq!(published.0, StatusCode::OK);
    let id = published.2["id"].as_i64().unwrap();
    assert_eq!(
        call(&app, "/admin/notices", Some(d.clone()), &admin, ORIGIN)
            .await
            .2["duplicate"],
        true
    );
    let mut changed = d.clone();
    changed["title"] = json!("changed");
    assert_eq!(
        call(&app, "/admin/notices", Some(changed), &admin, ORIGIN)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let mut fresh = d;
    fresh["request_id"] = json!("5e3f05f0-d4d6-4d48-9176-919d67ba5698");
    assert_eq!(
        call(&app, "/admin/notices", Some(fresh), &admin, ORIGIN)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        call(&app, "/notices", None, &one, ORIGIN).await.2["unread"],
        1
    );
    assert_eq!(
        call(&app, "/notices", None, &two, ORIGIN).await.2["unread"],
        0
    );
    assert_eq!(
        call(
            &app,
            &format!("/notices/{id}"),
            Some(json!({})),
            &two,
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let detail = call(
        &app,
        &format!("/notices/{id}"),
        Some(json!({})),
        &one,
        ORIGIN,
    )
    .await;
    assert_eq!(detail.0, StatusCode::OK);
    assert!(detail.2["html"].as_str().unwrap().contains("<h1>"));
    assert_eq!(
        call(&app, "/notices", None, &one, ORIGIN).await.2["unread"],
        0
    );
    let stats = call(&app, "/admin/notices", None, &admin, ORIGIN).await.2;
    assert_eq!(stats["items"][0]["recipients"], 1);
    assert_eq!(stats["items"][0]["read"], 1);
    assert!(!stats.to_string().contains("one@example"));
    assert_eq!(
        call(
            &app,
            &format!("/admin/notices/{id}/withdraw"),
            Some(json!({"password":PASSWORD})),
            &one,
            ORIGIN
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            &format!("/admin/notices/{id}/withdraw"),
            Some(json!({"password":PASSWORD})),
            &admin,
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &format!("/notices/{id}"),
            Some(json!({})),
            &one,
            ORIGIN
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        call(&app, "/notices", None, &one, ORIGIN).await.2["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn email_sync_requires_smtp_and_explicit_recipient_subscription() {
    let (_root, app) = setup();
    let (_, admin) = user(&app, "admin@example.invalid", true);
    app.db.grant_admin("admin@example.invalid").unwrap();
    let (id, one) = user(&app, "one@example.invalid", true);
    let (other, two) = user(&app, "two@example.invalid", true);
    let mut d = draft(vec![id, other]);
    d["email"] = json!(true);
    assert_eq!(
        call(&app, "/admin/notices", Some(d.clone()), &admin, ORIGIN)
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        call(&app, "/notices", None, &one, ORIGIN).await.2["email_enabled"],
        false
    );
    assert_eq!(
        call(
            &app,
            "/notices/preferences",
            Some(json!({"email_enabled":true})),
            &one,
            "https://evil.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/notices/preferences",
            Some(json!({"email_enabled":true})),
            &one,
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/admin/notices/preview", Some(d), &admin, ORIGIN)
            .await
            .2["email_recipients"],
        1
    );
    assert_eq!(
        call(&app, "/notices", None, &two, ORIGIN).await.2["email_enabled"],
        false
    );
    assert_eq!(
        call(
            &app,
            "/notices/preferences",
            Some(json!({"email_enabled":false})),
            &one,
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
}
#[tokio::test]
async fn registration_and_login_require_explicit_current_policy_acceptance() {
    let (_root, app) = setup();
    let (id, _) = user(&app, "member@example.invalid", true);
    let credentials = json!({"email":"member@example.invalid","password":PASSWORD});
    for path in ["/auth/login", "/auth/register"] {
        let result = call(&app, path, Some(credentials.clone()), "", ORIGIN).await;
        assert_eq!(result.0, StatusCode::BAD_REQUEST);
        assert!(result.1.get("set-cookie").is_none());
        let mut body = credentials.clone();
        body["accept_policies"] = json!(false);
        body["policy_version"] = json!(policy_version(&app.runtime.read().unwrap().settings.home));
        assert_eq!(
            call(&app, path, Some(body), "", ORIGIN).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let version = call(&app, "/policies", None, "", ORIGIN).await.2["version"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut accepted = credentials;
    accepted["accept_policies"] = json!(true);
    accepted["policy_version"] = json!(version);
    let result = call(&app, "/auth/login", Some(accepted.clone()), "", ORIGIN).await;
    assert_eq!(result.0, StatusCode::OK);
    assert!(result.1.get("set-cookie").is_some());
    let stored: i64 = app
        .db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM policy_acceptances WHERE user_id=? AND version=?",
            rusqlite::params![id, version],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, 1);
    app.runtime
        .write()
        .unwrap()
        .settings
        .home
        .terms
        .push_str("\n\nNew terms.");
    let stale = call(&app, "/auth/login", Some(accepted), "", ORIGIN).await;
    assert_eq!(stale.0, StatusCode::CONFLICT);
    assert!(stale.1.get("set-cookie").is_none());
    assert_ne!(
        call(&app, "/policies", None, "", ORIGIN).await.2["version"],
        version
    );
    app.runtime.write().unwrap().mailer = Some(web_ts::settings::Mailer {
        from: "site@example.invalid".parse().unwrap(),
        transport: lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(
            "127.0.0.1",
        )
        .port(9)
        .build(),
    });
    let mut registration = json!({"email":"new@example.invalid","password":PASSWORD,"accept_policies":true,"policy_version":version});
    assert_eq!(
        call(
            &app,
            "/auth/register",
            Some(registration.clone()),
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert!(app.db.password("new@example.invalid").unwrap().is_none());
    registration["policy_version"] =
        call(&app, "/policies", None, "", ORIGIN).await.2["version"].clone();
    assert_eq!(
        call(&app, "/auth/register", Some(registration), "", ORIGIN)
            .await
            .0,
        StatusCode::OK
    );
    let row = app.db.password("new@example.invalid").unwrap().unwrap();
    let saved: i64 = app
        .db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM policy_acceptances WHERE user_id=?",
            [row.0],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(saved, 1);
}

#[tokio::test]
async fn notification_worker_sends_only_one_recipient_and_persists_completion() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (_root, app) = setup();
    let (_, admin) = user(&app, "admin@example.invalid", true);
    app.db.grant_admin("admin@example.invalid").unwrap();
    let (id, one) = user(&app, "one@example.invalid", true);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sink = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        write
            .write_all(b"220 localhost test SMTP\r\n")
            .await
            .unwrap();
        let mut recipient = String::new();
        let mut data = false;
        let mut body = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            if data {
                if line == ".\r\n" {
                    write.write_all(b"250 accepted\r\n").await.unwrap();
                    break;
                }
                body.push_str(&line);
                continue;
            }
            if line.starts_with("RCPT TO:") {
                recipient.push_str(&line);
            }
            if line.starts_with("DATA") {
                data = true;
                write.write_all(b"354 send body\r\n").await.unwrap();
            } else {
                write.write_all(b"250 localhost\r\n").await.unwrap();
            }
        }
        (recipient, body)
    });
    // Unencrypted SMTP exists only inside this synthetic loopback test.
    app.runtime.write().unwrap().mailer = Some(web_ts::settings::Mailer {
        from: "site@example.invalid".parse().unwrap(),
        transport: lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(
            "127.0.0.1",
        )
        .port(port)
        .build(),
    });
    call(
        &app,
        "/notices/preferences",
        Some(json!({"email_enabled":true})),
        &one,
        ORIGIN,
    )
    .await;
    let mut d = draft(vec![id]);
    d["email"] = json!(true);
    assert_eq!(
        call(&app, "/admin/notices", Some(d), &admin, ORIGIN)
            .await
            .0,
        StatusCode::OK
    );
    web_ts::notices::start(app.clone());
    let (recipient, body) = tokio::time::timeout(std::time::Duration::from_secs(8), sink)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recipient.lines().count(), 1);
    assert!(recipient.contains("one@example.invalid"));
    assert!(!body.contains("Bcc:"));
    assert!(!body.contains("admin@example.invalid"));
    let mut done = false;
    for _ in 0..30 {
        let history = call(&app, "/admin/notices", None, &admin, ORIGIN).await.2;
        if history["items"][0]["sent"] == 1 {
            done = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(done, "SMTP completion must be persisted before another job");
}
