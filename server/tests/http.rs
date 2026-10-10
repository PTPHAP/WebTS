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
    // Normal credential flows explicitly accept the current published policies.
    let body = body.map(|mut value| {
        if matches!(path, "/auth/login" | "/auth/register") {
            value["accept_policies"] = json!(true);
            value["policy_version"] = json!(web_ts::site::policy_version(
                &app.runtime.read().unwrap().settings.home
            ));
        }
        value
    });
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
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn screen_capture_policy_blocks_this_origin_without_disabling_voice() {
    let (_directory, app) = setup();
    let (status, headers, _) = call(&app, "/health", None, "", "https://webts.example").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers["permissions-policy"],
        "display-capture=(), camera=(), microphone=(self)"
    );
    assert_eq!(headers["x-frame-options"], "DENY");
}
#[tokio::test]
async fn site_branding_policies_and_footer_are_admin_only_sanitized_and_preserved() {
    let (directory, app) = setup();
    let pw = "synthetic site admin password";
    let hash = password::hash(pw).unwrap();
    let origin = "https://webts.example";
    let mut cookies = Vec::new();
    for email in ["site-admin@example.com", "site-member@example.com"] {
        let (id, _, token) = app.db.register(email, &hash).unwrap();
        app.db.consume_email_token(&token, "verify", None).unwrap();
        cookies.push(format!(
            "__Host-webts={}",
            app.db.create_session(id, false, &hash).unwrap()
        ));
    }
    app.db.grant_admin("site-admin@example.com").unwrap();
    let mut image = std::io::Cursor::new(Vec::new());
    image::RgbImage::new(8, 8)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    use base64::Engine;
    let home = json!({"site_name":"Synthetic voice site","site_icon":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(image.into_inner())),"operator":"Synthetic operator","contact":"contact@example.com","data_details":"test environment only","footer_html":"<a href='https://beian.miit.gov.cn/' onclick='steal()'>备案号</a><img src='https://evil.example/track'><script>steal()</script>","privacy_policy":"## Privacy\n\nActual policy text","terms":"## Terms\n\nActual disclaimer text"});
    let body = json!({"password":pw,"home":home});
    assert_eq!(
        call(&app, "/admin/home", Some(body.clone()), &cookies[1], origin)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/admin/home",
            Some(body.clone()),
            &cookies[0],
            "https://evil.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/admin/home",
            Some(json!({"password":"wrong","home":home})),
            &cookies[0],
            origin
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "/admin/home", Some(body), &cookies[0], origin)
            .await
            .0,
        StatusCode::OK
    );
    let public = call(&app, "/site", None, "", origin).await.2;
    assert_eq!(public["site_name"], "Synthetic voice site");
    assert!(
        public["site_icon"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert!(
        public["footer_html"]
            .as_str()
            .unwrap()
            .contains("target=\"_blank\"")
    );
    assert!(!public.to_string().contains("steal()"));
    assert!(!public.to_string().contains("evil.example"));
    assert!(public.get("smtp").is_none());
    assert!(public.get("servers").is_none());
    // A pre-upgrade editor only sends old home fields; it must not erase branding or policies.
    assert_eq!(call(&app,"/admin/home",Some(json!({"password":pw,"home":{"title":"Legacy title","subtitle":"","image":"","announcements":[]}})),&cookies[0],origin).await.0,StatusCode::OK);
    let settings = call(&app, "/admin/settings", None, &cookies[0], origin)
        .await
        .2;
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(json!({"password":pw,"settings":settings})),
            &cookies[0],
            origin
        )
        .await
        .0,
        StatusCode::OK
    );
    let retained = call(&app, "/site", None, "", origin).await.2;
    assert_eq!(retained["site_name"], public["site_name"]);
    assert_eq!(retained["privacy_policy"], public["privacy_policy"]);
    assert_eq!(retained["terms"], public["terms"]);
    assert_eq!(retained["footer_html"], public["footer_html"]);
    let mut config = app.config.clone();
    config.database = directory.path().join("db").to_string_lossy().into_owned();
    let restarted = App::new(config).unwrap();
    assert_eq!(
        call(&restarted, "/site", None, "", origin).await.2,
        retained
    );
}

#[tokio::test]
async fn image_policy_is_admin_only_persisted_and_applied_to_profile_uploads() {
    let (_dir, app) = setup();
    let pw = "image policy fixture password";
    let hash = password::hash(pw).unwrap();
    let mut cookies = Vec::new();
    for email in ["image-admin@example.com", "image-member@example.com"] {
        let (id, _, token) = app.db.register(email, &hash).unwrap();
        app.db.consume_email_token(&token, "verify", None).unwrap();
        cookies.push(format!(
            "__Host-webts={}",
            app.db.create_session(id, false, &hash).unwrap()
        ));
    }
    app.db.grant_admin("image-admin@example.com").unwrap();
    let origin = "https://webts.example";
    let mut settings = call(&app, "/admin/settings", None, &cookies[0], origin)
        .await
        .2;
    settings["image_limits"]["avatar_upload_kib"] = json!(128);
    settings["image_limits"]["channel_images"] = json!(2);
    let update = json!({"password":pw,"settings":settings});
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(update.clone()),
            &cookies[1],
            origin
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut wrong = update.clone();
    wrong["password"] = json!("wrong password");
    assert_eq!(
        call(&app, "/admin/settings", Some(wrong), &cookies[0], origin)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let mut unsafe_update = update.clone();
    unsafe_update["settings"]["image_limits"]["channel_download_kib"] = json!(8193);
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(unsafe_update),
            &cookies[0],
            origin
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.runtime
            .read()
            .unwrap()
            .settings
            .image_limits
            .avatar_upload_kib,
        64
    );
    assert_eq!(
        call(&app, "/admin/settings", Some(update), &cookies[0], origin)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/servers", None, &cookies[1], origin).await.2["image_limits"]["channel_images"],
        2
    );
    let reloaded = App::new(app.config.clone()).unwrap();
    assert_eq!(
        reloaded
            .runtime
            .read()
            .unwrap()
            .settings
            .image_limits
            .avatar_upload_kib,
        128
    );
    let mut image = image::RgbImage::new(128, 256);
    let mut seed = 123_u32;
    for pixel in image.pixels_mut() {
        for byte in &mut pixel.0 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *byte = (seed >> 24) as u8;
        }
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    use base64::Engine;
    let avatar = base64::engine::general_purpose::STANDARD.encode(bytes.into_inner());
    let profile = json!({"display_name":"fixture","about":"","avatar":avatar,"sync_avatar":false,"sync_about":false});
    assert!(profile.to_string().len() > 128 * 1024);
    assert_eq!(
        call(&app, "/profile", Some(profile.clone()), &cookies[1], origin)
            .await
            .0,
        StatusCode::OK
    );
    let mut settings = call(&app, "/admin/settings", None, &cookies[0], origin)
        .await
        .2;
    settings["image_limits"]["avatar_upload_kib"] = json!(8);
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(json!({"password":pw,"settings":settings})),
            &cookies[0],
            origin
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/profile", Some(profile), &cookies[1], origin)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn page_links_support_direct_navigation_without_masking_missing_assets() {
    let (directory, mut app) = setup();
    let web = directory.path().join("web");
    std::fs::create_dir(&web).unwrap();
    let index = "<!doctype html><title>WebTS</title>";
    std::fs::write(web.join("index.html"), index).unwrap();
    std::sync::Arc::get_mut(&mut app).unwrap().config.web_dir = web.to_string_lossy().into_owned();
    for path in [
        "/",
        "/login",
        "/app",
        "/privacy",
        "/terms",
        "/assets/missing.js",
    ] {
        let response = router(app.clone())
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        if path.starts_with("/assets/") {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        } else {
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap(), index);
        }
    }
}

#[tokio::test]
async fn profiles_and_public_home_keep_ownership_and_admin_boundaries() {
    let (_dir, app) = setup();
    let pw = "profile fixture password";
    let hash = password::hash(pw).unwrap();
    let mut cookies = Vec::new();
    for email in ["home-admin@example.com", "profile-member@example.com"] {
        let (id, _, token) = app.db.register(email, &hash).unwrap();
        app.db.consume_email_token(&token, "verify", None).unwrap();
        cookies.push(format!(
            "__Host-webts={}",
            app.db.create_session(id, true, &hash).unwrap()
        ));
    }
    app.db.grant_admin("home-admin@example.com").unwrap();
    let origin = "https://webts.example";
    assert_eq!(
        call(&app, "/profile", None, "", origin).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "/admin/home", None, &cookies[1], origin).await.0,
        StatusCode::FORBIDDEN
    );
    let profile = json!({"display_name":"WebTS测试", "about":"<script>纯文本</script>", "avatar":"", "sync_avatar":true,"sync_about":true});
    assert_eq!(
        call(
            &app,
            "/profile",
            Some(profile.clone()),
            &cookies[1],
            "https://evil.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "/profile", Some(profile.clone()), &cookies[1], origin)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/profile", None, &cookies[1], origin).await.2,
        profile
    );
    assert_eq!(
        call(&app, "/profile", None, &cookies[0], origin).await.2["about"],
        ""
    );
    let mut forged = profile.clone();
    forged["user_id"] = json!(1);
    assert_eq!(
        call(&app, "/profile", Some(forged), &cookies[1], origin)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // A real raster larger than the normal 32KiB API limit exercises the dedicated upload route.
    let mut pixels = image::RgbImage::new(256, 256);
    let mut seed = 42_u32;
    for pixel in pixels.pixels_mut() {
        for byte in &mut pixel.0 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *byte = (seed >> 24) as u8;
        }
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 75)
        .encode_image(&pixels)
        .unwrap();
    use base64::Engine;
    let picture = format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    );
    assert!(picture.len() > 32768);
    let home = json!({"title":"新首页", "subtitle":"公开介绍", "image":picture,"announcements":[{"id":"visible","title":"公告","text":"<img src=x onerror=alert(1)>","image":"","enabled":true},{"id":"hidden","title":"未发布内容","text":"draft","image":"","enabled":false}]});
    assert_eq!(
        call(
            &app,
            "/admin/home",
            Some(json!({"password":pw,"home":home})),
            &cookies[1],
            origin
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/admin/home",
            Some(json!({"password":"wrong","home":home})),
            &cookies[0],
            origin
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "/admin/home",
            Some(json!({"password":pw,"home":home})),
            &cookies[0],
            origin
        )
        .await
        .0,
        StatusCode::OK
    );
    let published = call(&app, "/site", None, "", origin).await.2;
    assert_eq!(published["announcements"].as_array().unwrap().len(), 1);
    assert_eq!(published["title"], "新首页");
    assert!(!published.to_string().contains("smtp"));
    let basic = app.runtime.read().unwrap().settings.admin_view();
    assert_eq!(
        call(
            &app,
            "/admin/settings",
            Some(json!({"password":pw,"settings":basic})),
            &cookies[0],
            origin
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/site", None, "", origin).await.2["title"],
        "新首页"
    );
    let ciphertext = app.db.settings().unwrap().unwrap();
    let saved: web_ts::settings::Settings = serde_json::from_slice(
        &app.vault
            .open(0, "site-settings", "v1", &ciphertext)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved.home.title, "新首页");
    assert_eq!(
        call(&app, "/auth/logout", Some(json!({})), &cookies[1], origin)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/profile", Some(profile), &cookies[1], origin)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
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
