use crate::{
    config::Config,
    db::{Db, Session},
    gateway::Connections,
    identity, password,
    vault::Vault,
};
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use lettre::{AsyncTransport, Message};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};
use tokio::sync::{Semaphore, mpsc};
use tower_http::services::{ServeDir, ServeFile};
use zeroize::Zeroizing;

pub struct App {
    pub config: Config,
    pub db: Db,
    pub vault: Vault,
    pub connections: Arc<Connections>,
    pub sockets: Arc<Semaphore>,
    workers: Arc<Semaphore>,
    limits: Mutex<HashMap<(IpAddr, bool), (Instant, u32)>>,
    mail: mpsc::Sender<Mail>,
    pub runtime: Arc<RwLock<crate::settings::Runtime>>,
    dummy_hash: String,
}
struct Mail {
    email: String,
    purpose: &'static str,
    token: Zeroizing<String>,
}
pub struct Error(pub StatusCode, pub &'static str);
impl Error {
    pub fn bad(message: &'static str) -> Self {
        Self(StatusCode::BAD_REQUEST, message)
    }
}
impl From<anyhow::Error> for Error {
    fn from(_: anyhow::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "服务暂时不可用，请稍后重试",
        )
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
pub type Api<T> = std::result::Result<T, Error>;

impl App {
    pub fn new(config: Config) -> Result<Arc<Self>> {
        for path in [&config.database, &config.master_key_file] {
            if let Some(parent) = std::path::Path::new(path).parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
        }
        let text = Zeroizing::new(
            std::fs::read_to_string(&config.master_key_file)
                .context("请先通过init-key创建部署密钥；不会自动替换已有身份的密钥")?,
        );
        let bytes =
            Zeroizing::new(hex::decode(text.trim()).context("部署密钥必须为32字节十六进制")?);
        let key: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("部署密钥长度无效"))?;
        let db = Db::open(&config.database)?;
        let vault = Vault::new(key);
        let settings = if let Some(record) = db.settings()? {
            let plaintext = vault.open(0, "site-settings", "v1", &record)?;
            serde_json::from_slice(&plaintext)?
        } else {
            crate::settings::Settings {
                servers: config.servers.clone(),
                default_server: config
                    .servers
                    .first()
                    .map(|s| s.id.clone())
                    .unwrap_or_default(),
                allow_custom: false,
                smtp: config
                    .smtp
                    .as_ref()
                    .map(|smtp| -> Result<crate::settings::SmtpSettings> {
                        Ok(crate::settings::SmtpSettings {
                            host: smtp.host.clone(),
                            port: smtp.port,
                            username: smtp.username.clone(),
                            from: smtp.from.clone(),
                            password: Zeroizing::new(
                                std::fs::read_to_string(&smtp.password_file)
                                    .context("无法读取SMTP密码文件")?,
                            )
                            .trim_end()
                            .to_owned(),
                        })
                    })
                    .transpose()?,
            }
        };
        let runtime = Arc::new(RwLock::new(crate::settings::Runtime::new(settings)?));
        let (mail, mut rx) = mpsc::channel::<Mail>(32);
        let mail_runtime = runtime.clone();
        let origin = config.origin();
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let mailer = mail_runtime.read().unwrap().mailer.clone();
                let success = if let Some(mailer) = mailer {
                    let (subject, plain, html) =
                        crate::email::content(job.purpose, &origin, &job.token);
                    let message = job.email.parse().ok().and_then(|to| {
                        Message::builder()
                            .from(mailer.from)
                            .to(to)
                            .subject(subject)
                            .multipart(lettre::message::MultiPart::alternative_plain_html(
                                plain, html,
                            ))
                            .ok()
                    });
                    match message {
                        Some(message) => mailer.transport.send(message).await.is_ok(),
                        None => false,
                    }
                } else {
                    false
                };
                if !success {
                    tracing::warn!("SMTP发送失败；未记录邮箱、凭据或令牌");
                }
            }
        });
        Ok(Arc::new(Self {
            connections: Arc::new(Connections::new(config.max_connections)),
            sockets: Arc::new(Semaphore::new(config.max_connections)),
            config,
            db,
            vault,
            workers: Arc::new(Semaphore::new(2)),
            limits: Mutex::new(HashMap::new()),
            mail,
            runtime,
            dummy_hash: password::hash(&crate::vault::token()?)?,
        }))
    }
    pub async fn work<T: Send + 'static>(
        self: &Arc<Self>,
        f: impl FnOnce(&App) -> Api<T> + Send + 'static,
    ) -> Api<T> {
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "服务正忙，请稍后重试"))?;
        let app = self.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f(&app)
        })
        .await
        .map_err(|_| Error(StatusCode::INTERNAL_SERVER_ERROR, "操作失败"))?
    }
    pub fn session(&self, headers: &HeaderMap) -> Api<Session> {
        let cookie_name = self.cookie_name();
        let token = headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.split(';').find_map(|c| {
                    c.trim()
                        .split_once('=')
                        .filter(|(name, _)| *name == cookie_name)
                        .map(|(_, value)| value)
                })
            });
        token
            .and_then(|t| self.db.session(t).ok().flatten())
            .ok_or(Error(
                StatusCode::UNAUTHORIZED,
                "请先登录，或重新登录已过期的账号",
            ))
    }
    fn cookie_name(&self) -> &str {
        if self.config.secure_cookie() {
            "__Host-webts"
        } else {
            "webts_dev"
        }
    }
    fn cookie(&self, token: &str, remember: bool, clear: bool) -> String {
        format!(
            "{}={}; Path=/; HttpOnly; SameSite=Strict{}{}",
            self.cookie_name(),
            token,
            if self.config.secure_cookie() {
                "; Secure"
            } else {
                ""
            },
            if clear {
                "; Max-Age=0".to_owned()
            } else if remember {
                "; Max-Age=2592000".to_owned()
            } else {
                String::new()
            }
        )
    }
    fn smtp_ready(&self) -> bool {
        self.runtime.read().unwrap().mailer.is_some()
    }
    fn queue_mail(&self, email: String, purpose: &'static str, token: String) -> Api<()> {
        if !self.smtp_ready() {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "站点尚未配置邮箱服务，请联系站长",
            ));
        }
        self.mail
            .try_send(Mail {
                email,
                purpose,
                token: Zeroizing::new(token),
            })
            .map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "邮件队列繁忙，请稍后重试"))
    }
    pub(crate) fn reauthenticate(&self, headers: &HeaderMap, password: &str) -> Api<Session> {
        let session = self.session(headers)?;
        let hash = self.db.password_by_id(session.user.id)?;
        if !password::verify(password, &hash) {
            return Err(Error(StatusCode::UNAUTHORIZED, "当前密码不正确"));
        }
        self.session(headers)
    }
}

pub fn router(app: Arc<App>) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route(
            "/admin/settings",
            get(crate::settings::get_settings).post(crate::settings::save_settings),
        )
        .route("/me", get(me))
        .route("/admin/accounts", get(crate::accounts::list))
        .route(
            "/admin/accounts/{id}",
            get(crate::accounts::detail).post(crate::accounts::update),
        )
        .route("/auth/register", post(register))
        .route("/auth/resend", post(resend))
        .route("/auth/login", post(login))
        .route("/auth/verify", post(verify))
        .route("/auth/forgot", post(forgot))
        .route("/auth/reset", post(reset))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-all", post(logout_all))
        .route("/identities", get(list_identities).post(create_identity))
        .route("/identities/import", post(import_identity))
        .route("/identities/{id}", patch(edit_identity))
        .route("/identities/{id}/delete", post(delete_identity))
        .route("/identities/{id}/export", post(export_identity))
        .route("/servers", get(servers))
        .route("/rtc", get(crate::media::ice_config))
        .route("/connect", get(crate::gateway::upgrade))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(middleware::from_fn_with_state(app.clone(), guard));
    Router::new()
        .nest("/api", api)
        .fallback_service(
            ServeDir::new(&app.config.web_dir)
                .not_found_service(ServeFile::new(format!("{}/index.html", app.config.web_dir))),
        )
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}
async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("content-security-policy",HeaderValue::from_static("default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; media-src 'self' blob:; object-src 'none'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"));
    if api {
        h.insert("cache-control", HeaderValue::from_static("no-store"));
    }
    response
}
async fn guard(
    State(app): State<Arc<App>>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let mutation = !matches!(*request.method(), Method::GET | Method::HEAD);
    if (mutation || request.uri().path().ends_with("/connect"))
        && request
            .headers()
            .get("origin")
            .and_then(|v| v.to_str().ok())
            != Some(app.config.origin().as_str())
    {
        return Error(StatusCode::FORBIDDEN, "请求来源不被允许").into_response();
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let ip = if app.config.trusted_proxy.contains(&peer) {
        request
            .headers()
            .get("x-real-ip")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.parse().ok())
            .unwrap_or(peer)
    } else {
        peer
    };
    let auth = request.uri().path().contains("/auth/")
        || (mutation && request.uri().path().contains("/admin/"));
    let window = if auth { 600 } else { 60 };
    let cap = if auth { 30 } else { 240 };
    let limited = {
        let mut map = app.limits.lock().unwrap();
        map.retain(|_, (start, _)| start.elapsed().as_secs() < 600);
        if map.len() >= 4096 && !map.contains_key(&(ip, auth)) {
            true
        } else {
            let entry = map.entry((ip, auth)).or_insert((Instant::now(), 0));
            if entry.0.elapsed().as_secs() >= window {
                *entry = (Instant::now(), 0);
            }
            entry.1 += 1;
            entry.1 > cap
        }
    };
    if limited {
        return Error(StatusCode::TOO_MANY_REQUESTS, "请求过于频繁，请稍后重试").into_response();
    }
    next.run(request).await
}
fn email(value: &str) -> Api<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() > 254
        || !value.is_ascii()
        || value.contains(['\r', '\n', ' '])
        || value.parse::<lettre::Address>().is_err()
    {
        return Err(Error::bad("邮箱格式无效"));
    }
    Ok(value)
}
fn name(value: &str) -> Api<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 80 || value.chars().any(char::is_control) {
        return Err(Error::bad("名称需为1至80个字符"));
    }
    Ok(value.to_owned())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialsBody {
    email: String,
    password: String,
    #[serde(default)]
    remember: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmailBody {
    email: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenBody {
    token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetBody {
    token: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityBody {
    name: String,
    #[serde(default)]
    is_default: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportBody {
    name: String,
    identity: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordBody {
    password: String,
}
async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({"ok":true,"version":env!("CARGO_PKG_VERSION"),"smtp_ready":app.smtp_ready()}))
}
async fn me(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    Ok(Json(json!({"user":app.session(&headers)?.user})))
}
async fn register(
    State(app): State<Arc<App>>,
    Json(body): Json<CredentialsBody>,
) -> Api<Json<Value>> {
    if !app.smtp_ready() {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "站点尚未配置邮箱服务，请联系站长",
        ));
    }
    let email = email(&body.email)?;
    app.work(move |a| {
        let p = Zeroizing::new(body.password);
        let hash = password::hash(&p).map_err(|_| Error::bad("密码需要12至128字节"))?;
        let (_, verified, token) = a.db.register(&email, &hash)?;
        if !verified {
            a.queue_mail(email, "verify", token)?;
        }
        Ok(Json(
            json!({"message":"如果邮箱可注册，验证链接已加入邮件队列，请检查收件箱与垃圾邮件。"}),
        ))
    })
    .await
}
async fn login(State(app): State<Arc<App>>, Json(body): Json<CredentialsBody>) -> Api<Response> {
    let email = email(&body.email)?;
    app.work(move |a| {
        let found = a.db.password(&email)?;
        let stored = found
            .as_ref()
            .map(|(_, h, _)| h.as_str())
            .unwrap_or(&a.dummy_hash);
        let p = Zeroizing::new(body.password);
        let valid = password::verify(&p, stored);
        let Some((id, hash, true)) = found.filter(|_| valid) else {
            return Err(Error(
                StatusCode::UNAUTHORIZED,
                "邮箱或密码错误，或邮箱尚未验证",
            ));
        };
        let token = a.db.create_session(id, body.remember, &hash)?;
        let mut response =
            Json(json!({"user":a.db.session(&token)?.context("登录会话不存在")?.user}))
                .into_response();
        response.headers_mut().insert(
            "set-cookie",
            HeaderValue::from_str(&a.cookie(&token, body.remember, false)).unwrap(),
        );
        Ok(response)
    })
    .await
}
async fn verify(State(app): State<Arc<App>>, Json(body): Json<TokenBody>) -> Api<Json<Value>> {
    app.work(move |a| {
        a.db.consume_email_token(&body.token, "verify", None)
            .map_err(|_| Error::bad("验证链接无效或已过期"))?;
        Ok(Json(json!({"message":"邮箱已验证，现在可以登录。"})))
    })
    .await
}
async fn resend(State(app): State<Arc<App>>, Json(body): Json<EmailBody>) -> Api<Json<Value>> {
    if !app.smtp_ready() {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "站点尚未配置邮箱服务，请联系站长",
        ));
    }
    let email = email(&body.email)?;
    app.work(move |a| {
        let permit = a.mail.try_reserve().map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "邮件服务繁忙，请稍后再试"))?;
        if let Some(token) = a.db.resend_verification(&email)? { permit.send(Mail {email, purpose:"verify", token:Zeroizing::new(token)}); }
        Ok(Json(json!({"message":"如果账号需要验证，我们会发送一封新邮件。请检查收件箱与垃圾邮件，重复请求请间隔至少60秒。"})))
    }).await
}
async fn forgot(State(app): State<Arc<App>>, Json(body): Json<EmailBody>) -> Api<Json<Value>> {
    if !app.smtp_ready() {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "站点尚未配置邮箱服务，请联系站长",
        ));
    }
    let email = email(&body.email)?;
    app.work(move |a| {
        if a.mail.capacity() == 0 {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "邮件队列繁忙，请稍后重试",
            ));
        }
        if let Some((id, _, true)) = a.db.password(&email)? {
            let token = a.db.email_token(id, "reset")?;
            // Queue capacity can change after the check; keep unknown and known
            // accounts indistinguishable if another sender fills the last slot.
            let _ = a.queue_mail(email, "reset", token);
        }
        Ok(Json(
            json!({"message":"如果邮箱已验证，找回链接已加入邮件队列。"}),
        ))
    })
    .await
}
async fn reset(State(app): State<Arc<App>>, Json(body): Json<ResetBody>) -> Api<Json<Value>> {
    app.work(move |a| {
        let p = Zeroizing::new(body.password);
        let hash = password::hash(&p).map_err(|_| Error::bad("密码需要12至128字节"))?;
        let owner =
            a.db.consume_email_token(&body.token, "reset", Some(&hash))
                .map_err(|_| Error::bad("找回链接无效或已过期"))?;
        a.connections.cancel(owner, None, None);
        Ok(Json(
            json!({"message":"密码已重置，旧设备和连接已退出，身份已保留。"}),
        ))
    })
    .await
}
async fn end_session(app: Arc<App>, headers: HeaderMap, all: bool) -> Api<Response> {
    app.work(move |a| {
        let session = a.session(&headers)?;
        a.db.revoke(
            session.user.id,
            if all { None } else { Some(&session.hash) },
        )?;
        a.connections.cancel(
            session.user.id,
            if all { None } else { Some(&session.hash) },
            None,
        );
        let mut response = Json(json!({"ok":true})).into_response();
        response.headers_mut().insert(
            "set-cookie",
            HeaderValue::from_str(&a.cookie("", false, true)).unwrap(),
        );
        Ok(response)
    })
    .await
}
async fn logout(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Response> {
    end_session(app, headers, false).await
}
async fn logout_all(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Response> {
    end_session(app, headers, true).await
}
async fn list_identities(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a| {
        Ok(Json(
            json!({"identities":a.db.identities(a.session(&headers)?.user.id)?}),
        ))
    })
    .await
}
async fn create_identity(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<IdentityBody>,
) -> Api<Json<Value>> {
    let name = name(&body.name)?;
    app.work(move |a| {
        let owner = a.session(&headers)?.user.id;
        let value = tsclientlib::Identity::create();
        let id = a.db.add_identity(owner, &name, &value, &a.vault)?;
        if body.is_default {
            a.db.edit_identity(owner, &id, &name, true)?;
        }
        Ok(Json(json!({"id":id})))
    })
    .await
}
async fn import_identity(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<ImportBody>,
) -> Api<Json<Value>> {
    let name = name(&body.name)?;
    app.work(move |a| {
        let owner = a.session(&headers)?.user.id;
        let text = Zeroizing::new(body.identity);
        let value = identity::parse(&text)
            .map_err(|_| Error::bad("身份文件无效，需要TS客户端导出的私钥身份"))?;
        let id = a.db.add_identity(owner, &name, &value, &a.vault)?;
        Ok(Json(json!({"id":id})))
    })
    .await
}
async fn edit_identity(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<IdentityBody>,
) -> Api<Json<Value>> {
    let name = name(&body.name)?;
    app.work(move |a| {
        a.db.edit_identity(a.session(&headers)?.user.id, &id, &name, body.is_default)
            .map_err(|_| Error::bad("身份不存在或操作失败"))?;
        Ok(Json(json!({"ok":true})))
    })
    .await
}
async fn delete_identity(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PasswordBody>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let p = Zeroizing::new(body.password);
        let owner = a.reauthenticate(&headers, &p)?.user.id;
        a.db.delete_identity(owner, &id)
            .map_err(|_| Error::bad("身份不存在"))?;
        a.connections.cancel(owner, None, Some(&id));
        Ok(Json(json!({"ok":true})))
    })
    .await
}
async fn export_identity(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PasswordBody>,
) -> Api<Response> {
    app.work(move |a| {
        let p = Zeroizing::new(body.password);
        let owner = a.reauthenticate(&headers, &p)?.user.id;
        let record =
            a.db.identity(owner, &id)
                .map_err(|_| Error::bad("身份不存在"))?;
        let plaintext = a.vault.open(owner, &id, &record.uid, &record.ciphertext)?;
        let text = std::str::from_utf8(&plaintext).map_err(|_| Error::bad("身份数据无效"))?;
        let value = identity::parse(text)?;
        let exported = identity::export(&value, &record.name);
        let mut response = exported.to_string().into_response();
        response.headers_mut().insert(
            "content-type",
            HeaderValue::from_static("application/octet-stream"),
        );
        response.headers_mut().insert(
            "content-disposition",
            HeaderValue::from_static("attachment; filename=teamspeak-identity.ini"),
        );
        Ok(response)
    })
    .await
}
async fn servers(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.session(&headers)?;
    Ok(Json(app.runtime.read().unwrap().settings.public()))
}
