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
async fn account_admin_bans_are_persistent_revoke_sessions_and_preserve_identities() {
    let (_dir, app) = setup();
    let hash = password::hash("account administration password").unwrap();
    let mut ids = Vec::new();
    let mut cookies = Vec::new();
    for email in ["operator@example.com", "member@example.com"] {
        let (id, _, verify) = app.db.register(email, &hash).unwrap();
        app.db.consume_email_token(&verify, "verify", None).unwrap();
        ids.push(id);
        cookies.push(format!(
            "__Host-webts={}",
            app.db.create_session(id, true, &hash).unwrap()
        ));
    }
    app.db.grant_admin("operator@example.com").unwrap();
    let identity = app
        .db
        .add_identity(
            ids[1],
            "保留身份",
            &tsclientlib::Identity::create(),
            &app.vault,
        )
        .unwrap();
    let path = format!("/admin/accounts/{}", ids[1]);
    assert_eq!(
        call(&app, "/admin/accounts", None, "", "https://webts.example")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, &path, None, &cookies[1], "https://webts.example")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let ban = json!({"password":"account administration password","action":"ban","text":"测试封禁","seconds":3600});
    assert_eq!(
        call(
            &app,
            &path,
            Some(ban.clone()),
            &cookies[0],
            "https://evil.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut wrong = ban.clone();
    wrong["password"] = json!("wrong");
    assert_eq!(
        call(
            &app,
            &path,
            Some(wrong),
            &cookies[0],
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let mut invalid = ban.clone();
    invalid["seconds"] = json!(u32::MAX);
    assert_eq!(
        call(
            &app,
            &path,
            Some(invalid),
            &cookies[0],
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            &format!("/admin/accounts/{}", ids[0]),
            Some(ban.clone()),
            &cookies[0],
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&app, &path, Some(ban), &cookies[0], "https://webts.example")
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/me", None, &cookies[1], "https://webts.example")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert!(app.db.create_session(ids[1], true, &hash).is_err());
    assert_eq!(call(&app,"/auth/login",Some(json!({"email":"member@example.com","password":"account administration password","remember":true})),"","https://webts.example").await.0,StatusCode::UNAUTHORIZED);
    let restored = App::new(app.config.clone()).unwrap();
    assert!(restored.db.create_session(ids[1], false, &hash).is_err());
    let (_, _, view) = call(&app, &path, None, &cookies[0], "https://webts.example").await;
    assert_eq!(view["banned"], true);
    assert_eq!(view["session_count"], 0);
    assert_eq!(view["identity_count"], 1);
    assert_eq!(view["audit"][0]["action"], "ban");
    assert!(view.to_string().find("password_hash").is_none());
    assert!(view.to_string().find("ciphertext").is_none());
    let (_, _, list) = call(
        &app,
        "/admin/accounts?search=member&status=banned",
        None,
        &cookies[0],
        "https://webts.example",
    )
    .await;
    assert_eq!(list["total"], 1);
    assert_eq!(
        call(
            &app,
            "/admin/accounts?status=invalid",
            None,
            &cookies[0],
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for action in ["unban", "note", "role", "revoke"] {
        assert_eq!(call(&app,&path,Some(json!({"password":"account administration password","action":action,"text":"测试管理备注","is_admin":true})),&cookies[0],"https://webts.example").await.0,StatusCode::OK);
    }
    // Unbanning does not resurrect old login cookies or replace the original identity.
    assert_eq!(
        call(&app, "/me", None, &cookies[1], "https://webts.example")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let new_session = app.db.create_session(ids[1], false, &hash).unwrap();
    assert!(app.db.session(&new_session).unwrap().unwrap().user.is_admin);
    let stored = app.db.identity(ids[1], &identity).unwrap();
    assert!(
        app.vault
            .open(ids[1], &identity, &stored.uid, &stored.ciphertext)
            .is_ok()
    );
    let (_, _, detail) = call(&app, &path, None, &cookies[0], "https://webts.example").await;
    assert_eq!(detail["admin_note"], "测试管理备注");
    assert_eq!(detail["audit"].as_array().unwrap().len(), 5);
}
#[tokio::test]
async fn administrator_settings_are_redacted_hot_loaded_and_persistent() {
    let (_dir, app) = setup();
    let hash = password::hash("administrator test password").unwrap();
    let (owner, _, verify) = app.db.register("admin@example.com", &hash).unwrap();
    app.db.consume_email_token(&verify, "verify", None).unwrap();
    let session = app.db.create_session(owner, false, &hash).unwrap();
    let cookie = format!("__Host-webts={session}");
    assert_eq!(
        call(&app, "/admin/settings", None, "", "https://webts.example")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            None,
            &cookie,
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    app.db.grant_admin("admin@example.com").unwrap();
    let settings = json!({"servers":[{"id":"community","name":"Community","address":"ts.example.com:9987"}],"default_server":"community","allow_custom":true,"smtp":{"host":"smtp.example.com","port":465,"username":"sender@example.com","from":"WebTS <sender@example.com>","password":"private-smtp-test-value"}});
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(json!({"password":"wrong","settings":settings})),
            &cookie,
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(json!({"password":"administrator test password","settings":settings})),
            &cookie,
            "https://other.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(json!({"password":"administrator test password","settings":settings})),
            &cookie,
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, _, public) = call(&app, "/servers", None, &cookie, "https://webts.example").await;
    assert_eq!(public["default_server"], "community");
    assert_eq!(public["allow_custom"], true);
    assert!(public.get("smtp").is_none());
    assert_eq!(
        call(&app, "/health", None, "", "https://webts.example")
            .await
            .2["smtp_ready"],
        true
    );
    let (_, _, view) = call(
        &app,
        "/admin/settings",
        None,
        &cookie,
        "https://webts.example",
    )
    .await;
    assert_eq!(view["smtp"]["password_set"], true);
    assert!(view["smtp"].get("password").is_none());
    let stored = app.db.settings().unwrap().unwrap();
    assert!(
        !stored
            .windows(b"private-smtp-test-value".len())
            .any(|s| s == b"private-smtp-test-value")
    );
    let mut changed = settings.clone();
    changed["smtp"]["password"] = json!("");
    changed["allow_custom"] = json!(false);
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(json!({"password":"administrator test password","settings":changed})),
            &cookie,
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        app.runtime
            .read()
            .unwrap()
            .settings
            .smtp
            .as_ref()
            .unwrap()
            .password,
        "private-smtp-test-value"
    );
    let restored = App::new(app.config.clone()).unwrap();
    assert!(!restored.runtime.read().unwrap().settings.allow_custom);
    assert_eq!(
        restored.runtime.read().unwrap().settings.default_server,
        "community"
    );
    assert_eq!(
        restored
            .runtime
            .read()
            .unwrap()
            .settings
            .smtp
            .as_ref()
            .unwrap()
            .password,
        "private-smtp-test-value"
    );
    changed["default_server"] = json!("missing");
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(json!({"password":"administrator test password","settings":changed})),
            &cookie,
            "https://webts.example"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let record = app.db.settings().unwrap().unwrap();
    app.db.revoke(owner, None).unwrap();
    assert!(
        app.db
            .save_settings(owner, &web_ts::db::digest(&session), &record)
            .is_err()
    );
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
