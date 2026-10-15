use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use web_ts::{
    app::{App, router},
    config::Config,
    password,
};

const ORIGIN: &str = "https://security.example";
const PASSWORD: &str = "synthetic security center password";
fn fixture() -> (tempfile::TempDir, Arc<App>, String, String, String) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.cache");
    std::fs::create_dir_all(&root).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("sessions-test-")
        .tempdir_in(root)
        .unwrap();
    let key = dir.path().join("key");
    std::fs::write(&key, "42".repeat(32)).unwrap();
    let config: Config = toml::from_str(include_str!("../../config.example.toml")).unwrap();
    let app = App::new(Config {
        database: ":memory:".into(),
        master_key_file: key.to_string_lossy().into_owned(),
        public_url: ORIGIN.into(),
        ..config
    })
    .unwrap();
    let hash = password::hash(PASSWORD).unwrap();
    for email in ["session-a@example.invalid", "session-b@example.invalid"] {
        let (_, _, t) = app.db.register(email, &hash).unwrap();
        app.db.consume_email_token(&t, "verify", None).unwrap();
    }
    let first = app
        .db
        .create_session_with_client(1, false, &hash, "Edge", "Windows")
        .unwrap();
    let second = app.db.create_session(1, true, &hash).unwrap();
    let other = app.db.create_session(2, false, &hash).unwrap();
    (dir, app, first, second, other)
}
async fn call(
    app: &Arc<App>,
    path: &str,
    body: Option<Value>,
    token: &str,
    origin: &str,
) -> (StatusCode, HeaderMap, Value) {
    let request = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(format!("/api{path}"))
        .header("origin", origin)
        .header("cookie", format!("__Host-webts={token}"))
        .header("content-type", "application/json")
        .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
        .unwrap();
    let response = router(app.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn session_list_and_revocation_are_owned_scoped_reauthenticated_and_not_credentials() {
    let (_dir, app, first, second, other) = fixture();
    assert_eq!(
        call(&app, "/auth/sessions?account=1", None, "", ORIGIN)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "/auth/sessions?account=2", None, &first, ORIGIN)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (status, headers, rows) =
        call(&app, "/auth/sessions?account=1", None, &first, ORIGIN).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(rows["total"], 2);
    let items = rows["items"].as_array().unwrap();
    assert_eq!(items[0]["current"], true);
    assert_eq!(items[0]["browser"], "Edge");
    assert_eq!(items[0]["remembered"], false);
    let target = items[1]["id"].as_str().unwrap();
    for secret in [
        &first,
        &second,
        &other,
        &web_ts::db::digest(&first),
        &web_ts::db::digest(&second),
    ] {
        assert!(!rows.to_string().contains(secret));
    }
    assert_eq!(
        call(&app, "/me", None, target, ORIGIN).await.0,
        StatusCode::UNAUTHORIZED
    );
    let foreign = call(&app, "/auth/sessions?account=2", None, &other, ORIGIN)
        .await
        .2["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for (body, origin, expected) in [
        (
            json!({"account":2,"target":target,"password":PASSWORD}),
            ORIGIN,
            StatusCode::CONFLICT,
        ),
        (
            json!({"account":1,"target":target,"password":"wrong"}),
            ORIGIN,
            StatusCode::UNAUTHORIZED,
        ),
        (
            json!({"account":1,"target":foreign,"password":PASSWORD}),
            ORIGIN,
            StatusCode::NOT_FOUND,
        ),
        (
            json!({"account":1,"target":items[0]["id"],"password":PASSWORD}),
            ORIGIN,
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"account":1,"target":"' OR 1=1","password":PASSWORD}),
            ORIGIN,
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"account":1,"target":target,"password":PASSWORD}),
            "https://evil.example",
            StatusCode::FORBIDDEN,
        ),
    ] {
        assert_eq!(
            call(&app, "/auth/sessions/revoke", Some(body), &first, origin)
                .await
                .0,
            expected
        );
    }
    assert!(app.db.session(&second).unwrap().is_some());
    let body = json!({"account":1,"target":target,"password":PASSWORD});
    let (status, _, result) = call(
        &app,
        "/auth/sessions/revoke",
        Some(body.clone()),
        &first,
        ORIGIN,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["removed"], 1);
    assert!(app.db.session(&second).unwrap().is_none());
    assert!(app.db.session(&first).unwrap().is_some());
    assert!(app.db.session(&other).unwrap().is_some());
    assert_eq!(
        call(&app, "/auth/sessions/revoke", Some(body), &first, ORIGIN)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "/auth/sessions/revoke",
            Some(json!({"account":1,"target":"others","password":PASSWORD})),
            &second,
            ORIGIN
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn list_is_bounded_and_logout_others_keeps_current_and_foreign_sessions() {
    let (_dir, app, first, second, other) = fixture();
    let hash = app.db.password_by_id(1).unwrap();
    for _ in 0..53 {
        app.db.create_session(1, false, &hash).unwrap();
    }
    let rows = call(&app, "/auth/sessions?account=1", None, &first, ORIGIN)
        .await
        .2;
    assert_eq!(rows["total"], 55);
    assert_eq!(rows["items"].as_array().unwrap().len(), 50);
    assert_eq!(rows["items"][0]["current"], true);
    let body = json!({"account":1,"target":"others","password":PASSWORD});
    let (status, headers, result) = call(
        &app,
        "/auth/sessions/revoke",
        Some(body.clone()),
        &first,
        ORIGIN,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["removed"], 54);
    assert!(!headers.contains_key("set-cookie"));
    assert!(app.db.session(&first).unwrap().is_some());
    assert!(app.db.session(&second).unwrap().is_none());
    assert!(app.db.session(&other).unwrap().is_some());
    assert_eq!(
        call(&app, "/auth/sessions/revoke", Some(body), &first, ORIGIN)
            .await
            .2["removed"],
        0
    );
    let details: i64 = app
        .db
        .connection
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM session_details", [], |r| r.get(0))
        .unwrap();
    assert_eq!(details, 2);
}

#[tokio::test]
async fn actual_login_records_only_coarse_labels_and_expired_sessions_are_hidden() {
    let (_dir, app, first, _second, _other) = fixture();
    let policy = web_ts::site::policy_version(&app.runtime.read().unwrap().settings.home);
    let body = json!({"email":"session-a@example.invalid","password":PASSWORD,"remember":true,"accept_policies":true,"policy_version":policy});
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header("origin", ORIGIN)
        .header("content-type", "application/json")
        .header(
            "user-agent",
            "Mozilla Windows Chrome/123 Safari/456 Edg/789 private-marker",
        )
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router(app.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    let token = cookie
        .split(';')
        .next()
        .unwrap()
        .strip_prefix("__Host-webts=")
        .unwrap();
    let rows = call(&app, "/auth/sessions?account=1", None, token, ORIGIN)
        .await
        .2;
    let current = &rows["items"][0];
    assert_eq!(current["browser"], "Edge");
    assert_eq!(current["platform"], "Windows");
    assert_eq!(current["remembered"], true);
    assert!(current["created_at"].as_i64().unwrap() > 0);
    assert!(!rows.to_string().contains("private-marker"));
    app.db
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE sessions SET expires=0 WHERE hash=?",
            [web_ts::db::digest(&first)],
        )
        .unwrap();
    let rows = call(&app, "/auth/sessions?account=1", None, token, ORIGIN)
        .await
        .2;
    assert_eq!(rows["total"], 2);
    assert!(
        rows["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["expires"].as_i64().unwrap() > web_ts::db::now())
    );
}

#[tokio::test]
#[ignore = "requires an explicitly authorized isolated TS3 server"]
async fn live_session_revocation_preserves_current_and_foreign_ts_connections() {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::{
        connect_async,
        tungstenite::{Message, client::IntoClientRequest},
    };
    let target = std::env::var("WEBTS_TEST_TARGET").expect("set authorized isolated TS3 target");
    let (_dir, app, current, other, foreign) = fixture();
    app.runtime
        .write()
        .unwrap()
        .settings
        .servers
        .push(web_ts::config::Server {
            id: "isolated".into(),
            name: "Session revocation test".into(),
            address: target,
        });
    let ids: Vec<_> = [1, 1, 2]
        .into_iter()
        .enumerate()
        .map(|(i, owner)| {
            app.db
                .add_identity(
                    owner,
                    &format!("session fixture {i}"),
                    &tsclientlib::Identity::create(),
                    &app.vault,
                )
                .unwrap()
        })
        .collect();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(
        axum::serve(
            listener,
            router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let mut sockets = Vec::new();
    for (i, token) in [&current, &other, &foreign].into_iter().enumerate() {
        let mut request = format!("ws://127.0.0.1:{port}/api/connect")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", ORIGIN.parse().unwrap());
        request
            .headers_mut()
            .insert("cookie", format!("__Host-webts={token}").parse().unwrap());
        let (mut socket, _) = connect_async(request).await.unwrap();
        socket.send(Message::Text(json!({"server":"isolated","identity":ids[i],"page":format!("session-{i}"),"name":format!("WebTS session test {i}")}).to_string().into())).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                if let Some(Ok(Message::Text(text))) = socket.next().await {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(value["type"], "error", "{value}");
                    assert_ne!(value["type"], "disconnected", "{value}");
                    if value["type"] == "state" {
                        break;
                    }
                } else {
                    panic!("connection closed before state");
                }
            }
        })
        .await
        .unwrap();
        sockets.push(socket);
    }
    assert_eq!(app.connections.count(1), 2);
    assert_eq!(app.connections.count(2), 1);
    let rows = call(&app, "/auth/sessions?account=1", None, &current, ORIGIN)
        .await
        .2;
    let public_id = rows["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["current"] == false)
        .unwrap()["id"]
        .clone();
    let result = call(
        &app,
        "/auth/sessions/revoke",
        Some(json!({"account":1,"target":public_id,"password":PASSWORD})),
        &current,
        ORIGIN,
    )
    .await;
    assert_eq!(result.0, StatusCode::OK);
    assert_eq!(result.2["removed"], 1);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = sockets[1].next().await {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] == "disconnected" {
                    assert_eq!(value["retryable"], false);
                    break;
                }
            } else {
                panic!("missing non-retryable disconnect event");
            }
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while app.connections.count(1) != 1 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(app.db.session(&current).unwrap().is_some());
    assert!(app.db.session(&foreign).unwrap().is_some());
    assert_eq!(app.connections.count(2), 1);
    // A new login after the first revocation remains usable until explicitly revoked.
    let hash = app.db.password_by_id(1).unwrap();
    let next = app.db.create_session(1, false, &hash).unwrap();
    let mut request = format!("ws://127.0.0.1:{port}/api/connect")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", ORIGIN.parse().unwrap());
    request
        .headers_mut()
        .insert("cookie", format!("__Host-webts={next}").parse().unwrap());
    let (mut replacement, _) = connect_async(request).await.unwrap();
    replacement.send(Message::Text(json!({"server":"isolated","identity":ids[1],"page":"session-replacement","name":"WebTS replacement test"}).to_string().into())).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            if let Some(Ok(Message::Text(text))) = replacement.next().await {
                let value: Value = serde_json::from_str(&text).unwrap();
                assert_ne!(value["type"], "error", "{value}");
                assert_ne!(value["type"], "disconnected", "{value}");
                if value["type"] == "state" {
                    break;
                }
            } else {
                panic!("replacement closed before state");
            }
        }
    })
    .await
    .unwrap();
    let result = call(
        &app,
        "/auth/sessions/revoke",
        Some(json!({"account":1,"target":"others","password":PASSWORD})),
        &current,
        ORIGIN,
    )
    .await;
    assert_eq!(result.0, StatusCode::OK);
    assert_eq!(result.2["removed"], 1);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = replacement.next().await {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] == "disconnected" {
                    assert_eq!(value["retryable"], false);
                    break;
                }
            } else {
                panic!("missing replacement disconnect event");
            }
        }
    })
    .await
    .unwrap();
    assert!(app.db.session(&current).unwrap().is_some());
    assert!(app.db.session(&foreign).unwrap().is_some());
    assert_eq!(app.connections.count(2), 1);
    for i in [0, 2] {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(Ok(Message::Text(text))) = sockets[i].next().await {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(
                        value["type"], "disconnected",
                        "preserved session disconnected: {value}"
                    );
                    if value["type"] == "heartbeat" {
                        break;
                    }
                } else {
                    panic!("preserved session closed");
                }
            }
        })
        .await
        .unwrap();
        sockets[i].close(None).await.unwrap();
    }
    server.abort();
    println!(
        "PASS: selected and all-other session revocation disconnect real isolated TS3 actors; current and foreign sessions retain their connections"
    );
}
