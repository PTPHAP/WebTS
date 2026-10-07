use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use web_ts::{
    app::{App, router},
    config::Config,
    password,
};

fn setup() -> (tempfile::TempDir, std::sync::Arc<App>) {
    let directory = tempfile::tempdir().unwrap();
    let key = directory.path().join("master.key");
    std::fs::write(&key, hex::encode([42; 32])).unwrap();
    let config: Config = toml::from_str(include_str!("../../config.example.toml")).unwrap();
    let app = App::new(Config {
        database: directory.path().join("db").to_string_lossy().into_owned(),
        master_key_file: key.to_string_lossy().into_owned(),
        public_url: "https://webts.example".into(),
        ..config
    })
    .unwrap();
    (directory, app)
}
async fn call(
    app: &std::sync::Arc<App>,
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
        .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
        .unwrap();
    let response = router(app.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 32768).await.unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn cookie_origin_and_owner_boundaries_and_reset() {
    let (_dir, app) = setup();
    let hash = password::hash("a sufficiently long password").unwrap();
    let (owner, _, verify) = app.db.register("one@example.com", &hash).unwrap();
    assert_eq!(
        call(
            &app,
            "/auth/login",
            Some(json!({"email":"one@example.com","password":"a sufficiently long password"})),
            "",
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    app.db.consume_email_token(&verify, "verify", None).unwrap();
    let(login,headers,_)=call(&app,"/auth/login",Some(json!({"email":"one@example.com","password":"a sufficiently long password","remember":true})),"","https://webts.example").await;
    assert_eq!(login, StatusCode::OK);
    let cookie = headers["set-cookie"].to_str().unwrap().to_owned();
    assert!(cookie.starts_with("__Host-webts="));
    for flag in ["Secure", "HttpOnly", "SameSite=Strict", "Max-Age=2592000"] {
        assert!(cookie.contains(flag));
    }
    assert_eq!(
        call(
            &app,
            "/identities",
            Some(json!({"name":"mine"})),
            &cookie,
            "https://attacker.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, _, created) = call(
        &app,
        "/identities",
        Some(json!({"name":"mine"})),
        &cookie,
        "https://webts.example",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = created["id"].as_str().unwrap();
    let (other, _, verify) = app.db.register("two@example.com", &hash).unwrap();
    app.db.consume_email_token(&verify, "verify", None).unwrap();
    let other_cookie = format!(
        "__Host-webts={}",
        app.db.create_session(other, false, &hash).unwrap()
    );
    let path = format!("/identities/{id}/export");
    assert_eq!(
        call(
            &app,
            &path,
            Some(json!({"password":"a sufficiently long password"})),
            &other_cookie,
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            &path,
            Some(json!({"password":"wrong"})),
            &cookie,
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let token = app.db.email_token(owner, "reset").unwrap();
    let request = json!({"token":token,"password":"a new sufficiently long password"});
    assert_eq!(
        call(
            &app,
            "/auth/reset",
            Some(request.clone()),
            "",
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/me", None, &cookie, "https://webts.example")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(app.db.identities(owner).unwrap().len(), 1);
    assert_eq!(
        call(
            &app,
            "/auth/reset",
            Some(request),
            "",
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "/auth/register",
            Some(json!({"email":"new@example.com","password":"long enough password"})),
            "",
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}
