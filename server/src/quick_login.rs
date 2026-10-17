//! Short-lived, two-sided login approval. QR possession alone never authenticates a browser.
use crate::{
    app::{Api, App, Error},
    db::{digest, now},
    preferences::session_valid,
    vault::token,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::Zeroizing;

pub fn login_name(value: &str) -> Api<String> {
    let name = value.trim().to_ascii_lowercase();
    if !(4..=32).contains(&name.len())
        || !name.as_bytes()[0].is_ascii_lowercase()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(Error::bad(
            "登录用户名需4至32位英文字母、数字或下划线，并以字母开头",
        ));
    }
    Ok(name)
}
pub(crate) fn restricted(path: &str, mutation: bool) -> bool {
    path.starts_with("/admin/")
        || matches!(
            path,
            "/auth/qr/inspect" | "/auth/qr/approve" | "/auth/logout-all" | "/auth/sessions/revoke"
        )
        || mutation
            && (path == "/auth/username"
                || path.starts_with("/identities")
                || matches!(
                    path,
                    "/friends/key" | "/friends/key/reset" | "/friends/device"
                )
                || path.ends_with("/prekey")
                || path.ends_with("/encryption"))
}
fn unavailable() -> Error {
    Error(
        StatusCode::NOT_FOUND,
        "登录请求无效、已使用或已过期，请重新生成二维码",
    )
}
fn secret(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn full_session(a: &App, h: &HeaderMap) -> Api<crate::db::Session> {
    let s = a.session(h)?;
    if s.user.public_device {
        return Err(Error(StatusCode::FORBIDDEN, "请在已登录的个人设备上确认"));
    }
    Ok(s)
}
fn policies<'a>(
    a: &'a App,
    accept: bool,
    version: &str,
) -> Api<std::sync::RwLockReadGuard<'a, crate::settings::Runtime>> {
    if !accept {
        return Err(Error::bad("请先阅读并同意隐私政策与使用协议及免责声明"));
    }
    let runtime = a.runtime.read().unwrap();
    if version != crate::site::policy_version(&runtime.settings.home) {
        return Err(Error(StatusCode::CONFLICT, "协议已更新，请重新阅读并同意"));
    }
    Ok(runtime)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameBody {
    account: i64,
    name: String,
    password: String,
}
pub async fn get_name(State(app): State<Arc<App>>, h: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a| {
        let s = a.session(&h)?;
        let db = a.db.connection.lock().unwrap();
        session_valid(&db, &s)?;
        let name: Option<String> = db
            .query_row(
                "SELECT name FROM login_names WHERE user_id=?",
                [s.user.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(anyhow::Error::from)?;
        Ok(Json(
            json!({"account":s.user.id,"name":name.unwrap_or_default()}),
        ))
    })
    .await
}
pub async fn set_name(
    State(app): State<Arc<App>>,
    h: HeaderMap,
    Json(body): Json<NameBody>,
) -> Api<Json<Value>> {
    let name = login_name(&body.name)?;
    if body.password.len() > 128 {
        return Err(Error::bad("密码长度无效"));
    }
    app.expensive(move|a|{let s=a.reauthenticate(&h,&Zeroizing::new(body.password))?;if s.user.id!=body.account{return Err(Error(StatusCode::CONFLICT,"账号已变化"));}let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        if db.query_row("SELECT EXISTS(SELECT 1 FROM login_names WHERE name=? AND user_id!=?)",params![name,s.user.id],|r|r.get::<_,bool>(0)).map_err(anyhow::Error::from)?{return Err(Error(StatusCode::CONFLICT,"该登录用户名不可用"));}
        db.execute("INSERT INTO login_names VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET name=excluded.name",params![s.user.id,name]).map_err(anyhow::Error::from)?;Ok(Json(json!({"account":s.user.id,"name":name})))
    }).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consent {
    accept_policies: bool,
    policy_version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    id: String,
    secret: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    account: i64,
    id: String,
    secret: String,
    code: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    account: i64,
    id: String,
    secret: String,
    accept_policies: bool,
    policy_version: String,
}
fn checked(body: &Ticket) -> Api<()> {
    if !secret(&body.id) || !secret(&body.secret) {
        return Err(unavailable());
    }
    Ok(())
}
pub async fn start(
    State(app): State<Arc<App>>,
    h: HeaderMap,
    Json(body): Json<Consent>,
) -> Api<Json<Value>> {
    app.work(move|a|{
        let _policy=policies(a,body.accept_policies,&body.policy_version)?;if a.session(&h).is_ok(){return Err(Error(StatusCode::CONFLICT,"当前已登录，请先退出再切换账号"));}
        let id=token()?;let poll=token()?;let approve=token()?;let code=token()?[..4].to_ascii_uppercase();let expires=now()+120;let (browser,platform)=crate::sessions::client_label(&h);
        let db=a.db.connection.lock().unwrap();db.execute("DELETE FROM quick_logins WHERE expires<=?",[now()]).map_err(anyhow::Error::from)?;
        if db.query_row("SELECT count(*) FROM quick_logins",[],|r|r.get::<_,i64>(0)).map_err(anyhow::Error::from)?>=1000{return Err(Error(StatusCode::SERVICE_UNAVAILABLE,"扫码登录繁忙，请稍后重试"));}
        db.execute("INSERT INTO quick_logins(id,poll_hash,approve_hash,code,expires,policy,browser,platform) VALUES(?,?,?,?,?,?,?,?)",params![id,digest(&poll),digest(&approve),code,expires,body.policy_version,browser,platform]).map_err(anyhow::Error::from)?;
        let url=format!("{}/login#qr={id}.{approve}",a.config.origin());
        Ok(Json(json!({"id":id,"secret":poll,"url":url,"code":code,"expires":expires,"server_time":now()})))
    }).await
}
pub async fn poll(State(app): State<Arc<App>>, Json(body): Json<Ticket>) -> Api<Json<Value>> {
    checked(&body)?;
    app.work(move|a|{
        let db=a.db.connection.lock().unwrap();let row:Option<Option<String>>=db.query_row("SELECT authorizer FROM quick_logins WHERE id=? AND poll_hash=? AND expires>?",params![body.id,digest(&body.secret),now()],|r|r.get(0)).optional().map_err(anyhow::Error::from)?;let auth=row.ok_or_else(unavailable)?;
        a.quick_login_limit(&body.id,false)?;
        let Some(hash)=auth else{return Ok(Json(json!({"status":"pending"})));};
        let owner:i64=db.query_row("SELECT s.user_id FROM sessions s JOIN users u ON u.id=s.user_id JOIN session_details d ON d.hash=s.hash WHERE s.hash=? AND s.expires>? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=? AND d.public_device=0",params![hash,now(),now()],|r|r.get(0)).optional().map_err(anyhow::Error::from)?.ok_or_else(unavailable)?;
        let name:String=db.query_row("SELECT COALESCE((SELECT json_extract(data,'$.display_name') FROM profiles WHERE user_id=?),'WebTS 用户')",[owner],|r|r.get(0)).map_err(anyhow::Error::from)?;
        Ok(Json(json!({"status":"approved","name":name,"account":owner})))
    }).await
}
pub async fn inspect(
    State(app): State<Arc<App>>,
    h: HeaderMap,
    Json(body): Json<Ticket>,
) -> Api<Json<Value>> {
    checked(&body)?;
    app.work(move|a|{let s=full_session(a,&h)?;let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        let (browser,platform,expires):(String,String,i64)=db.query_row("SELECT browser,platform,expires FROM quick_logins WHERE id=? AND approve_hash=? AND expires>? AND authorizer IS NULL",params![body.id,digest(&body.secret),now()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(anyhow::Error::from)?.ok_or_else(unavailable)?;
        Ok(Json(json!({"account":s.user.id,"browser":browser,"platform":platform,"expires":expires,"origin":a.config.origin(),"public_device":true})))
    }).await
}
pub async fn approve(
    State(app): State<Arc<App>>,
    h: HeaderMap,
    Json(body): Json<Approval>,
) -> Api<Json<Value>> {
    checked(&Ticket {
        id: body.id.clone(),
        secret: body.secret.clone(),
    })?;
    if body.code.len() != 4 {
        return Err(Error::bad("请输入电脑上显示的4位核对码"));
    }
    app.work(move|a|{let s=full_session(a,&h)?;let runtime=a.runtime.read().unwrap();let db=a.db.connection.lock().unwrap();session_valid(&db,&s)?;
        if s.user.id!=body.account{return Err(Error(StatusCode::CONFLICT,"账号已变化，请重新核对扫码请求"));}
        let row:Option<(String,String,Option<String>)>=db.query_row("SELECT code,policy,authorizer FROM quick_logins WHERE id=? AND approve_hash=? AND expires>?",params![body.id,digest(&body.secret),now()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(anyhow::Error::from)?;let(code,policy,authorizer)=row.ok_or_else(unavailable)?;
        a.quick_login_limit(&body.id,true)?;
        if code!=body.code.to_ascii_uppercase(){return Err(Error::bad("核对码不一致，请确认正在登录的是面前这台电脑"));}
        if policy!=crate::site::policy_version(&runtime.settings.home){return Err(Error(StatusCode::CONFLICT,"协议已更新，请重新生成二维码"));}
        if authorizer.as_ref().is_some_and(|v|v!=&s.hash){return Err(Error(StatusCode::CONFLICT,"该请求已由其他登录确认，不能更换账号"));}
        db.execute("UPDATE quick_logins SET authorizer=? WHERE id=?",params![s.hash,body.id]).map_err(anyhow::Error::from)?;Ok(Json(json!({"account":s.user.id,"approved":true})))
    }).await
}
pub async fn cancel(State(app): State<Arc<App>>, Json(body): Json<Ticket>) -> Api<Json<Value>> {
    checked(&body)?;
    app.work(move |a| {
        a.db.connection
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM quick_logins WHERE id=? AND poll_hash=?",
                params![body.id, digest(&body.secret)],
            )
            .map_err(anyhow::Error::from)?;
        Ok(Json(json!({"cancelled":true})))
    })
    .await
}
pub async fn claim(
    State(app): State<Arc<App>>,
    h: HeaderMap,
    Json(body): Json<Claim>,
) -> Api<Response> {
    checked(&Ticket {
        id: body.id.clone(),
        secret: body.secret.clone(),
    })?;
    app.work(move|a|{
        let _policy=policies(a,body.accept_policies,&body.policy_version)?;if a.session(&h).is_ok(){return Err(Error(StatusCode::CONFLICT,"当前已登录，请先退出再切换账号"));}
        let credential=Zeroizing::new(token()?);let mut db=a.db.connection.lock().unwrap();let tx=db.transaction().map_err(anyhow::Error::from)?;
        let owner:i64=tx.query_row("SELECT s.user_id FROM quick_logins q JOIN sessions s ON s.hash=q.authorizer JOIN users u ON u.id=s.user_id JOIN session_details d ON d.hash=s.hash WHERE q.id=? AND q.poll_hash=? AND q.expires>? AND q.policy=? AND s.expires>? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=? AND d.public_device=0",params![body.id,digest(&body.secret),now(),body.policy_version,now(),now()],|r|r.get(0)).optional().map_err(anyhow::Error::from)?.ok_or_else(unavailable)?;
        if owner!=body.account{return Err(Error(StatusCode::CONFLICT,"授权账号与电脑显示不一致，请重新生成二维码"));}
        let(browser,platform):(String,String)=tx.query_row("SELECT browser,platform FROM quick_logins WHERE id=?",[&body.id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(anyhow::Error::from)?;
        tx.execute("DELETE FROM quick_logins WHERE id=?",[body.id]).map_err(anyhow::Error::from)?;
        tx.execute("DELETE FROM sessions WHERE expires<=?",[now()]).map_err(anyhow::Error::from)?;
        tx.execute("INSERT INTO sessions VALUES(?,?,?)",params![digest(&credential),owner,now()+4*3600]).map_err(anyhow::Error::from)?;
        tx.execute("INSERT INTO session_details(hash,id,created_at,browser,platform,remembered,public_device) VALUES(?,?,?,?,?,0,1)",params![digest(&credential),token()?,now(),browser,platform]).map_err(anyhow::Error::from)?;
        tx.execute("UPDATE users SET last_login=? WHERE id=?",params![now(),owner]).map_err(anyhow::Error::from)?;
        tx.execute("INSERT INTO policy_acceptances VALUES(?,?,?) ON CONFLICT(user_id,version) DO UPDATE SET accepted_at=excluded.accepted_at",params![owner,body.policy_version,now()]).map_err(anyhow::Error::from)?;
        tx.commit().map_err(anyhow::Error::from)?;drop(db);
        let user=a.db.session(&credential)?.ok_or_else(unavailable)?.user;let mut response=Json(json!({"user":user})).into_response();response.headers_mut().insert("set-cookie",HeaderValue::from_str(&a.cookie(&credential,false,false)).unwrap());Ok(response)
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request};
    use tower::ServiceExt;
    fn policy(a: &App) -> String {
        crate::site::policy_version(&a.runtime.read().unwrap().settings.home)
    }
    fn consent(a: &App) -> Consent {
        Consent {
            accept_policies: true,
            policy_version: policy(a),
        }
    }
    fn ticket(v: &Value, approval: bool) -> Ticket {
        let secret = if approval {
            v["url"].as_str().unwrap().rsplit('.').next().unwrap()
        } else {
            v["secret"].as_str().unwrap()
        };
        Ticket {
            id: v["id"].as_str().unwrap().into(),
            secret: secret.into(),
        }
    }
    async fn issue(a: &Arc<App>) -> Value {
        start(State(a.clone()), HeaderMap::new(), Json(consent(a)))
            .await
            .unwrap()
            .0
    }
    async fn allow(a: &Arc<App>, h: &HeaderMap, v: &Value) {
        let t = ticket(v, true);
        let _ = approve(
            State(a.clone()),
            h.clone(),
            Json(Approval {
                account: a.session(h).unwrap().user.id,
                id: t.id,
                secret: t.secret,
                code: v["code"].as_str().unwrap().into(),
            }),
        )
        .await
        .unwrap();
    }
    fn claim_body(a: &App, v: &Value) -> Claim {
        let t = ticket(v, false);
        Claim {
            account: 1,
            id: t.id,
            secret: t.secret,
            accept_policies: true,
            policy_version: policy(a),
        }
    }
    async fn route(a: &Arc<App>, path: &str, body: Option<Value>, h: &HeaderMap) -> Response {
        let mut r = Request::builder()
            .method(if body.is_some() {
                Method::POST
            } else {
                Method::GET
            })
            .uri(path)
            .body(
                body.map(|v| Body::from(v.to_string()))
                    .unwrap_or_else(Body::empty),
            )
            .unwrap();
        *r.headers_mut() = h.clone();
        if r.method() == Method::POST {
            r.headers_mut()
                .insert("content-type", HeaderValue::from_static("application/json"));
        }
        crate::app::router(a.clone()).oneshot(r).await.unwrap()
    }
    #[tokio::test]
    async fn qr_separates_secrets_requires_both_confirmations_and_is_single_use() {
        let (_dir, a, h) = crate::app::tests::fixture();
        let v = issue(&a).await;
        assert_eq!(
            poll(State(a.clone()), Json(ticket(&v, false)))
                .await
                .unwrap()
                .0["status"],
            "pending"
        );
        assert_eq!(
            poll(State(a.clone()), Json(ticket(&v, true)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            inspect(State(a.clone()), h.clone(), Json(ticket(&v, false)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            inspect(State(a.clone()), HeaderMap::new(), Json(ticket(&v, true)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
        let preview = inspect(State(a.clone()), h.clone(), Json(ticket(&v, true)))
            .await
            .unwrap()
            .0;
        assert_eq!(preview["account"], 1);
        assert!(!preview.to_string().contains(v["code"].as_str().unwrap()));
        assert!(preview.get("email").is_none());
        let t = ticket(&v, true);
        let wrong = if v["code"] == "AAAA" { "BBBB" } else { "AAAA" };
        assert_eq!(
            approve(
                State(a.clone()),
                h.clone(),
                Json(Approval {
                    account: 1,
                    id: t.id,
                    secret: t.secret,
                    code: wrong.into()
                })
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::BAD_REQUEST
        );
        allow(&a, &h, &v).await;
        allow(&a, &h, &v).await;
        assert_eq!(
            poll(State(a.clone()), Json(ticket(&v, false)))
                .await
                .unwrap()
                .0["status"],
            "approved"
        );
        let response = claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
            .await
            .unwrap();
        let cookie = response.headers()["set-cookie"].to_str().unwrap();
        assert!(!cookie.contains("Max-Age"));
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
        let mut pc = HeaderMap::new();
        pc.insert("cookie", cookie.split(';').next().unwrap().parse().unwrap());
        let s = a.session(&pc).unwrap();
        assert_eq!(s.user.id, 1);
        assert!(s.user.public_device);
        assert!((s.expires - now() - 4 * 3600).abs() <= 1);
        let data: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(data["user"]["public_device"], true);
        assert_eq!(
            claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
        assert!(a.session(&h).is_ok());
        assert_eq!(
            a.db.connection
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM quick_logins", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[tokio::test]
    async fn qr_rechecks_revocation_ban_expiry_policy_and_cannot_be_reassigned() {
        let (_dir, a, h) = crate::app::tests::fixture();
        let v = issue(&a).await;
        allow(&a, &h, &v).await;
        let other = a.db.create_session(2, false, "fixture hash").unwrap();
        let mut other_h = h.clone();
        other_h.insert("cookie", format!("webts_dev={other}").parse().unwrap());
        let t = ticket(&v, true);
        assert_eq!(
            approve(
                State(a.clone()),
                other_h,
                Json(Approval {
                    account: 2,
                    id: t.id,
                    secret: t.secret,
                    code: v["code"].as_str().unwrap().into()
                })
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
            .execute("UPDATE users SET banned_until=-1 WHERE id=1", [])
            .unwrap();
        assert_eq!(
            claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=0 WHERE id=1", [])
            .unwrap();
        a.runtime.write().unwrap().settings.home.site_name = "changed-policy".into();
        assert!(
            claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
                .await
                .is_err()
        );
        let v = issue(&a).await;
        allow(&a, &h, &v).await;
        a.db.connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE quick_logins SET expires=? WHERE id=?",
                params![now() - 1, v["id"].as_str().unwrap()],
            )
            .unwrap();
        assert_eq!(
            claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
        let v = issue(&a).await;
        allow(&a, &h, &v).await;
        a.db.revoke(1, None).unwrap();
        assert_eq!(
            claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::NOT_FOUND
        );
    }
    #[tokio::test]
    async fn simultaneous_claims_have_one_winner_and_cancel_is_idempotent() {
        let (_dir, a, h) = crate::app::tests::fixture();
        let v = issue(&a).await;
        allow(&a, &h, &v).await;
        let (one, two) = tokio::join!(
            claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v))),
            claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
        );
        assert_ne!(one.is_ok(), two.is_ok());
        let v = issue(&a).await;
        let _ = cancel(State(a.clone()), Json(ticket(&v, false)))
            .await
            .unwrap();
        let _ = cancel(State(a.clone()), Json(ticket(&v, false)))
            .await
            .unwrap();
        assert!(
            poll(State(a.clone()), Json(ticket(&v, false)))
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn public_device_is_server_enforced_even_with_correct_password_or_admin_role() {
        let (_dir, a, h) = crate::app::tests::fixture();
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=1 WHERE id=1", [])
            .unwrap();
        let v = issue(&a).await;
        allow(&a, &h, &v).await;
        let response = claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
            .await
            .unwrap();
        let mut pc = h.clone();
        pc.insert(
            "cookie",
            response.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .parse()
                .unwrap(),
        );
        assert_eq!(
            a.reauthenticate(&pc, "anything").err().unwrap().0,
            StatusCode::FORBIDDEN
        );
        assert!(
            a.db.create_session_with_security(1, true, "fixture hash", "", "", true)
                .is_err()
        );
        for (path, body) in [
            ("/api/admin/health", None),
            (
                "/api/auth/qr/inspect",
                Some(json!({"id":"a".repeat(64),"secret":"b".repeat(64)})),
            ),
            (
                "/api/auth/username",
                Some(json!({"account":1,"name":"test_user","password":"anything"})),
            ),
            (
                "/api/identities/import",
                Some(json!({"name":"test","identity":"test"})),
            ),
            ("/api/friends/device", Some(json!({}))),
            ("/api/auth/logout-all", Some(json!({}))),
        ] {
            assert_eq!(
                route(&a, path, body, &pc).await.status(),
                StatusCode::FORBIDDEN,
                "{path}"
            );
        }
        assert!(!a.session(&pc).unwrap().user.is_admin);
        a.runtime.write().unwrap().settings.community_enabled = true;
        assert_eq!(
            route(&a, "/api/community?account=1&kind=report", None, &pc)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            route(&a, "/api/me", None, &pc).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            route(&a, "/api/identities", None, &pc).await.status(),
            StatusCode::OK
        );
        let t = ticket(&v, true);
        assert_eq!(
            inspect(State(a.clone()), pc.clone(), Json(t))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::FORBIDDEN
        );
        let session = a.session(&pc).unwrap();
        a.db.revoke(1, Some(&session.hash)).unwrap();
        assert!(a.session(&pc).is_err());
        assert!(a.session(&h).is_ok());
    }
    #[tokio::test]
    async fn username_is_unique_normalized_account_scoped_and_email_remains_available() {
        let (_dir, a, h) = crate::app::tests::fixture();
        let password = "synthetic-long-password";
        let hash = crate::password::hash(password).unwrap();
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET password_hash=?", [hash])
            .unwrap();
        let change = |account, name: &str, password: &str| NameBody {
            account,
            name: name.into(),
            password: password.into(),
        };
        assert_eq!(
            set_name(
                State(a.clone()),
                h.clone(),
                Json(change(1, "Sample_User", password))
            )
            .await
            .unwrap()
            .0["name"],
            "sample_user"
        );
        assert_eq!(
            get_name(State(a.clone()), h.clone()).await.unwrap().0["name"],
            "sample_user"
        );
        assert_eq!(a.db.password("sample_user").unwrap().unwrap().0, 1);
        assert_eq!(a.db.password("audit-a@example.com").unwrap().unwrap().0, 1);
        assert_eq!(
            set_name(
                State(a.clone()),
                h.clone(),
                Json(change(2, "other_user", password))
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            set_name(
                State(a.clone()),
                h.clone(),
                Json(change(1, "other_user", "incorrect"))
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::UNAUTHORIZED
        );
        let other =
            a.db.create_session(2, false, &a.db.password_by_id(2).unwrap())
                .unwrap();
        let mut oh = h.clone();
        oh.insert("cookie", format!("webts_dev={other}").parse().unwrap());
        assert_eq!(
            set_name(
                State(a.clone()),
                oh,
                Json(change(2, "SAMPLE_USER", password))
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::CONFLICT
        );
        for invalid in [
            "",
            "abc",
            "12345",
            "bad@email.com",
            "name-here",
            "测试用户",
            "name\nuser",
            "user<script>",
        ] {
            assert!(login_name(invalid).is_err(), "{invalid}");
        }
        let response=route(&a,"/api/auth/login",Some(json!({"email":"SAMPLE_USER","password":password,"public_device":true,"accept_policies":true,"policy_version":policy(&a)})),&HeaderMap::from_iter([("origin".parse().unwrap(),HeaderValue::from_static("http://localhost:8080"))])).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            !response.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .contains("Max-Age")
        );
    }
    #[tokio::test]
    async fn qr_routes_reject_csrf_unknown_fields_and_do_not_store_bearer_secrets() {
        let (_dir, a, h) = crate::app::tests::fixture();
        let v = issue(&a).await;
        let t = ticket(&v, false);
        let body = json!({"id":t.id,"secret":t.secret});
        assert_eq!(
            route(
                &a,
                "/api/auth/qr/poll",
                Some(body.clone()),
                &HeaderMap::new()
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let mut anonymous = h.clone();
        anonymous.remove("cookie");
        let mut wrong = body.clone();
        wrong["owner"] = json!(2);
        assert_eq!(
            route(&a, "/api/auth/qr/poll", Some(wrong), &anonymous)
                .await
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let (db_poll, db_approve): (String, String) =
            a.db.connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT poll_hash,approve_hash FROM quick_logins WHERE id=?",
                    [v["id"].as_str().unwrap()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
        assert_ne!(db_poll, v["secret"].as_str().unwrap());
        assert_ne!(db_approve, ticket(&v, true).secret);
        assert_ne!(db_poll, db_approve);
        for _ in 0..30 {
            let _ = poll(State(a.clone()), Json(ticket(&v, false)))
                .await
                .unwrap();
        }
        assert_eq!(
            poll(State(a.clone()), Json(ticket(&v, false)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::TOO_MANY_REQUESTS
        );
        let other = issue(&a).await;
        assert!(
            poll(State(a.clone()), Json(ticket(&other, false)))
                .await
                .is_ok()
        );
    }
    #[tokio::test]
    async fn https_qr_cookie_has_host_secure_http_only_and_no_persistent_age() {
        let (_dir, mut a, mut h) = crate::app::tests::fixture();
        Arc::get_mut(&mut a).unwrap().config.public_url = "https://auth.example.invalid".into();
        let cookie = h["cookie"]
            .to_str()
            .unwrap()
            .replacen("webts_dev=", "__Host-webts=", 1);
        h.insert("cookie", cookie.parse().unwrap());
        h.insert(
            "origin",
            HeaderValue::from_static("https://auth.example.invalid"),
        );
        let v = issue(&a).await;
        assert!(
            v["url"]
                .as_str()
                .unwrap()
                .starts_with("https://auth.example.invalid/login#qr=")
        );
        allow(&a, &h, &v).await;
        let r = claim(State(a.clone()), HeaderMap::new(), Json(claim_body(&a, &v)))
            .await
            .unwrap();
        let cookie = r.headers()["set-cookie"].to_str().unwrap();
        assert!(cookie.starts_with("__Host-webts="));
        assert!(cookie.contains("; Secure"));
        assert!(cookie.contains("HttpOnly; SameSite=Strict"));
        assert!(!cookie.contains("Max-Age"));
        assert!(!cookie.contains("Domain="));
    }
    #[tokio::test]
    async fn expired_grants_are_bounded_and_swept_without_touching_accounts() {
        let (_dir, a, h) = crate::app::tests::fixture();
        let first = issue(&a).await;
        {
            let db = a.db.connection.lock().unwrap();
            for i in 1..1000 {
                db.execute("INSERT INTO quick_logins(id,poll_hash,approve_hash,code,expires,policy,browser,platform) VALUES(?,'hash','hash','AB12',?,'p','','')",params![format!("{i:064x}"),now()+120]).unwrap();
            }
        }
        assert_eq!(
            start(State(a.clone()), HeaderMap::new(), Json(consent(&a)))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE quick_logins SET expires=?", [now() - 1])
            .unwrap();
        let second = issue(&a).await;
        assert!(
            poll(State(a.clone()), Json(ticket(&first, false)))
                .await
                .is_err()
        );
        assert!(
            poll(State(a.clone()), Json(ticket(&second, false)))
                .await
                .is_ok()
        );
        assert!(a.session(&h).is_ok());
        assert_eq!(
            a.db.connection
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM quick_logins", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[tokio::test]
    async fn checked_policy_cannot_change_until_the_transaction_finishes() {
        let (_dir, a, _h) = crate::app::tests::fixture();
        let version = policy(&a);
        let guard = policies(&a, true, &version).unwrap();
        assert!(a.runtime.try_write().is_err());
        drop(guard);
        assert!(a.runtime.try_write().is_ok());
        assert!(policies(&a, true, "outdated").is_err());
        assert!(a.runtime.try_write().is_ok());
    }
    #[tokio::test]
    async fn account_switch_after_phone_inspection_must_not_authorize_the_new_cookie_owner() {
        let (_dir, a, h) = crate::app::tests::fixture();
        let v = issue(&a).await;
        let viewed = inspect(State(a.clone()), h.clone(), Json(ticket(&v, true)))
            .await
            .unwrap()
            .0;
        assert_eq!(viewed["account"], 1);
        let token = a.db.create_session(2, false, "fixture hash").unwrap();
        let mut changed = h.clone();
        changed.insert("cookie", format!("webts_dev={token}").parse().unwrap());
        let t = ticket(&v, true);
        let result = approve(
            State(a.clone()),
            changed,
            Json(Approval {
                account: 1,
                id: t.id,
                secret: t.secret,
                code: v["code"].as_str().unwrap().into(),
            }),
        )
        .await;
        assert!(
            result.is_err(),
            "phone confirmation must not silently follow a different account Cookie"
        );
        assert_eq!(
            poll(State(a.clone()), Json(ticket(&v, false)))
                .await
                .unwrap()
                .0["status"],
            "pending"
        );
    }
}
