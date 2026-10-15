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
const ORIGIN: &str = "https://reports.example.invalid";
fn setup() -> (tempfile::TempDir, Arc<App>) {
    let root = tempfile::tempdir().unwrap();
    let key = root.path().join("key");
    std::fs::write(&key, hex::encode([4; 32])).unwrap();
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
fn user(app: &App, n: u32) -> (i64, String) {
    let hash = password::hash("report-fixture-password").unwrap();
    let (id, _, token) = app
        .db
        .register(&format!("user{n}@example.invalid"), &hash)
        .unwrap();
    app.db.consume_email_token(&token, "verify", None).unwrap();
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
    origin: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(format!("/api{path}"))
        .header("cookie", cookie)
        .header("origin", origin)
        .header("content-type", "application/json")
        .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
        .unwrap();
    let r = router(app.clone()).oneshot(req).await.unwrap();
    let status = r.status();
    let bytes = to_bytes(r.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn draft(owner: i64, target: i64) -> Value {
    json!({"account":owner,"kind":"account","target":target.to_string(),"reason":"骚扰\n<script>alert(1)</script>","request_id":"2e3f05f0-d4d6-4d48-9176-919d67ba5698"})
}
#[tokio::test]
async fn report_limits_invalid_inputs_and_atomic_decisions() {
    let (_root, app) = setup();
    let (a, one) = user(&app, 10);
    let (b, _) = user(&app, 11);
    let (c, admin) = user(&app, 12);
    app.db.grant_admin("user12@example.invalid").unwrap();
    app.db
        .connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO friendships(lo,hi,status,requester,created_at) VALUES(?,?,2,?,?)",
            rusqlite::params![a, b, a, web_ts::db::now()],
        )
        .unwrap();
    for path in [
        "/reports",
        "/reports?account=1&before=-1",
        "/reports?account=1&extra=1",
    ] {
        assert_eq!(
            call(&app, path, None, &one, ORIGIN).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let mut bad = draft(a, b);
    bad["target"] = json!("1 OR 1=1");
    assert_eq!(
        call(&app, "/reports", Some(bad), &one, ORIGIN).await.0,
        StatusCode::NOT_FOUND
    );
    let mut bad = draft(a, b);
    bad["reason"] = json!("a".repeat(4001));
    assert_eq!(
        call(&app, "/reports", Some(bad), &one, ORIGIN).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut ids = Vec::new();
    for n in 0..5 {
        let mut v = draft(a, b);
        v["request_id"] = json!(format!("2e3f05f0-d4d6-4d48-9176-919d67ba56{n:02}"));
        let r = call(&app, "/reports", Some(v), &one, ORIGIN).await;
        assert_eq!(r.0, StatusCode::OK);
        ids.push(r.1["id"].as_str().unwrap().to_owned());
    }
    assert_eq!(
        call(&app, "/reports", Some(draft(a, b)), &one, ORIGIN)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let path = format!("/admin/reports/{}", ids[0]);
    let action = json!({"account":c,"revision":0,"status":"resolved","reply":"处理完成","note":"内部","request_id":"2e3f05f0-d4d6-4d48-9176-919d67ba5699"});
    let mut different = action.clone();
    different["request_id"] = json!("2e3f05f0-d4d6-4d48-9176-919d67ba5698");
    let (one, two) = tokio::join!(
        call(&app, &path, Some(action), &admin, ORIGIN),
        call(&app, &path, Some(different), &admin, ORIGIN)
    );
    let mut codes = [one.0.as_u16(), two.0.as_u16()];
    codes.sort();
    assert_eq!(codes, [200, 409]);
    let db = app.db.connection.lock().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM report_audit WHERE report_id=?",
            [&ids[0]],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn retired_event_reports_remain_private_and_new_activity_routes_are_disabled() {
    let (_root, app) = setup();
    let (a, one) = user(&app, 20);
    let (b, two) = user(&app, 21);
    let id = "c".repeat(64);
    let record = app
        .vault
        .seal(
            a,
            &id,
            "report-v1",
            serde_json::to_string(
                &json!({"reason":"legacy event evidence","reply":"old reply","note":"admin-only"}),
            )
            .unwrap()
            .as_bytes(),
        )
        .unwrap();
    app.db.connection.lock().unwrap().execute("INSERT INTO reports(id,owner,kind,target,request_id,request_hash,ciphertext,created_at,updated_at) VALUES(?,?,'event','legacy-event','legacy-request','hash',?,?,?)",rusqlite::params![id,a,record,web_ts::db::now(),web_ts::db::now()]).unwrap();
    let own = call(&app, &format!("/reports?account={a}"), None, &one, ORIGIN).await;
    assert_eq!(own.0, StatusCode::OK);
    assert_eq!(own.1["items"][0]["reason"], "legacy event evidence");
    assert!(own.1["items"][0]["note"].is_null());
    assert_eq!(
        call(&app, &format!("/reports?account={b}"), None, &two, ORIGIN)
            .await
            .1["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(call(&app,"/reports",Some(json!({"account":a,"kind":"event","target":"legacy-event","reason":"new event report","request_id":"2e3f05f0-d4d6-4d48-9176-919d67ba5698"})),&one,ORIGIN).await.0,StatusCode::BAD_REQUEST);
    for (path, body) in [
        ("/events", None),
        ("/events", Some(json!({}))),
        ("/events/legacy", Some(json!({"action":"cancel"}))),
    ] {
        let expected = if body.is_some() {
            StatusCode::METHOD_NOT_ALLOWED
        } else {
            StatusCode::NOT_FOUND
        };
        assert_eq!(call(&app, path, body, &one, ORIGIN).await.0, expected);
    }
}
#[tokio::test]
async fn reports_isolate_owners_encrypt_evidence_and_recheck_admin() {
    let (_root, app) = setup();
    let (a, one) = user(&app, 1);
    let (b, two) = user(&app, 2);
    let (c, other) = user(&app, 3);
    app.db
        .connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO friendships(lo,hi,status,requester,created_at) VALUES(?,?,1,?,?)",
            rusqlite::params![a, b, a, web_ts::db::now()],
        )
        .unwrap();
    assert_eq!(
        call(&app, "/reports", Some(draft(a, c)), &one, ORIGIN)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "/reports", Some(draft(c, b)), &one, ORIGIN)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "/reports",
            Some(draft(a, b)),
            &one,
            "https://evil.invalid"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let created = call(&app, "/reports", Some(draft(a, b)), &one, ORIGIN).await;
    assert_eq!(created.0, StatusCode::OK);
    let id = created.1["id"].as_str().unwrap();
    assert_eq!(
        call(&app, "/reports", Some(draft(a, b)), &one, ORIGIN)
            .await
            .1["id"],
        id
    );
    let mut changed = draft(a, b);
    changed["reason"] = json!("changed");
    assert_eq!(
        call(&app, "/reports", Some(changed), &one, ORIGIN).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, &format!("/reports?account={b}"), None, &two, ORIGIN)
            .await
            .1["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let ciphertext: Vec<u8> = app
        .db
        .connection
        .lock()
        .unwrap()
        .query_row("SELECT ciphertext FROM reports WHERE id=?", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!String::from_utf8_lossy(&ciphertext).contains("alert"));
    assert!(app.vault.open(b, id, "report-v1", &ciphertext).is_err());
    assert_eq!(
        call(
            &app,
            &format!("/admin/reports?account={c}"),
            None,
            &other,
            ORIGIN
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    app.db.grant_admin("user3@example.invalid").unwrap();
    let action = json!({"account":c,"revision":0,"status":"resolved","reply":"已处理","note":"private-admin-note","request_id":"2e3f05f0-d4d6-4d48-9176-919d67ba5699"});
    let path = format!("/admin/reports/{id}");
    assert_eq!(
        call(&app, &path, Some(action.clone()), &other, ORIGIN)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, &path, Some(action.clone()), &other, ORIGIN)
            .await
            .0,
        StatusCode::OK
    );
    let mut conflict = action.clone();
    conflict["request_id"] = json!("2e3f05f0-d4d6-4d48-9176-919d67ba5697");
    assert_eq!(
        call(&app, &path, Some(conflict), &other, ORIGIN).await.0,
        StatusCode::CONFLICT
    );
    let own = call(&app, &format!("/reports?account={a}"), None, &one, ORIGIN)
        .await
        .1;
    assert_eq!(own["items"][0]["status"], "resolved");
    assert_eq!(own["items"][0]["reply"], "已处理");
    assert!(!own.to_string().contains("private-admin-note"));
    let admin = call(
        &app,
        &format!("/admin/reports?account={c}"),
        None,
        &other,
        ORIGIN,
    )
    .await
    .1;
    assert_eq!(admin["items"][0]["audit"].as_array().unwrap().len(), 1);
    assert_eq!(admin["items"][0]["note"], "private-admin-note");
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE users SET is_admin=0 WHERE id=?", [c])
        .unwrap();
    assert_eq!(
        call(&app, &path, Some(action), &other, ORIGIN).await.0,
        StatusCode::FORBIDDEN
    );
    app.db
        .connection
        .lock()
        .unwrap()
        .execute("DELETE FROM sessions WHERE user_id=?", [a])
        .unwrap();
    assert_eq!(
        call(&app, &format!("/reports?account={a}"), None, &one, ORIGIN)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
