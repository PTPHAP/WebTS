//! Optional TS-colocated gateway. Only machine-authenticated signalling crosses
//! the site; WebRTC media terminates here. No account database is copied here.
use crate::{
    app::{Api, App, Error},
    config::{Config, Rtc},
};
use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{Query, State, WebSocketUpgrade},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::Response,
    routing::get,
};
use futures::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{
    Message, client::IntoClientRequest, protocol::WebSocketConfig,
};
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};

pub(crate) mod reverse;
pub mod setup;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteNode {
    pub id: String,
    pub server: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub reverse: bool,
    pub key_file: String,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn endpoint(value: &str, insecure: bool) -> Result<url::Url> {
    let url = url::Url::parse(value)?;
    let local = url.host_str().is_some_and(|host| {
        host.trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    });
    if !(url.scheme() == "https" || (insecure && local && url.scheme() == "http"))
        || url.host_str().is_none()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("节点必须使用无凭据、无路径的HTTPS地址；HTTP只允许显式开发模式的回环IP");
    }
    Ok(url)
}
fn key_from_text(text: &str) -> Result<Zeroizing<[u8; 32]>> {
    let bytes = Zeroizing::new(hex::decode(text.trim()).context("节点密钥格式无效")?);
    let key: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("节点密钥必须为32字节"))?;
    if key == [0; 32] {
        bail!("节点密钥不能全为零");
    }
    Ok(Zeroizing::new(key))
}
pub(crate) fn validate_bindings(config: &Config) -> Result<()> {
    if config.voice_nodes.len() > 32 {
        bail!("最多配置32个语音节点");
    }
    let mut ids = HashSet::new();
    let mut servers = HashSet::new();
    for node in &config.voice_nodes {
        if !valid_id(&node.id)
            || !ids.insert(&node.id)
            || !servers.insert(&node.server)
            || !config.servers.iter().any(|s| s.id == node.server)
            || node.key_file.is_empty()
            || node.key_file == config.master_key_file
        {
            bail!("节点ID/服务器绑定无效、重复或误用部署主密钥");
        }
        if node.reverse {
            if !node.url.is_empty() {
                bail!("反向节点不填写公网节点URL");
            }
        } else {
            endpoint(&node.url, config.allow_insecure_localhost)?;
        }
        key_from_text(&Zeroizing::new(
            std::fs::read_to_string(&node.key_file).context("无法读取节点配对密钥")?,
        ))?;
    }
    Ok(())
}
pub(crate) fn binding<'a>(
    app: &'a App,
    server: &str,
    source: &str,
) -> Result<Option<&'a RemoteNode>> {
    let node = app.config.voice_nodes.iter().find(|n| n.server == server);
    if node.is_some()
        && !app
            .config
            .servers
            .iter()
            .any(|s| s.id == server && s.address == source)
    {
        bail!("节点绑定地址已变化，请管理员重新配置；不会自动改走其他网关");
    }
    Ok(node)
}

fn signature(key: &[u8], id: &str, path: &str, time: i64, nonce: &str) -> Hmac<sha2::Sha256> {
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC accepts 32 byte keys");
    mac.update(format!("webts-node-v1\n{id}\nGET\n{path}\n{time}\n{nonce}").as_bytes());
    mac
}
fn signed_headers(key: &[u8], id: &str, path: &str) -> Result<HeaderMap> {
    let time = crate::db::now();
    let nonce = crate::vault::token()?;
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("x-webts-node-id", id.to_owned()),
        ("x-webts-node-time", time.to_string()),
        ("x-webts-node-nonce", nonce.clone()),
        (
            "authorization",
            format!(
                "WebTSNode {}",
                hex::encode(
                    signature(key, id, path, time, &nonce)
                        .finalize()
                        .into_bytes()
                )
            ),
        ),
    ] {
        headers.insert(name, HeaderValue::from_str(&value)?);
    }
    Ok(headers)
}
struct Authentication {
    id: String,
    key: Zeroizing<[u8; 32]>,
    seen: Mutex<HashMap<String, i64>>,
}
impl Authentication {
    fn verify(&self, headers: &HeaderMap, path: &str, now: i64) -> Result<()> {
        if headers.contains_key("origin") || headers.contains_key("cookie") {
            bail!("machine requests only");
        }
        let get = |name| -> Result<&str> {
            headers
                .get(name)
                .and_then(|h| h.to_str().ok())
                .context("missing machine header")
        };
        if get("x-webts-node-id")? != self.id {
            bail!("wrong node");
        }
        let time: i64 = get("x-webts-node-time")?.parse()?;
        let nonce = get("x-webts-node-nonce")?;
        if time.abs_diff(now) > 30
            || nonce.len() != 64
            || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
        {
            bail!("expired or malformed authentication");
        }
        let mac = hex::decode(
            get("authorization")?
                .strip_prefix("WebTSNode ")
                .context("invalid scheme")?,
        )?;
        signature(self.key.as_ref(), &self.id, path, time, nonce)
            .verify_slice(&mac)
            .map_err(|_| anyhow::anyhow!("invalid authentication"))?;
        let mut seen = self.seen.lock().unwrap();
        seen.retain(|_, at| now.abs_diff(*at) <= 60);
        if seen.len() >= 8192 || seen.contains_key(nonce) {
            bail!("replayed or busy");
        }
        seen.insert(nonce.to_owned(), now);
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    pub id: String,
    pub bind: SocketAddr,
    #[serde(default)]
    pub public_url: String,
    #[serde(default)]
    pub main_url: Option<String>,
    pub target: SocketAddr,
    pub file_transfer_port: u16,
    pub key_file: String,
    pub max_connections: usize,
    #[serde(default)]
    pub allow_insecure_localhost: bool,
    pub rtc: Rtc,
    #[serde(default)]
    pub image_limits: crate::settings::ImageLimits,
}
impl NodeConfig {
    pub fn load(path: &str) -> Result<Self> {
        let value: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<()> {
        if !valid_id(&self.id)
            || !self.target.ip().is_loopback()
            || self.target.port() == 0
            || self.file_transfer_port == 0
            || !self.bind.ip().is_loopback()
            || self.bind.port() == 0
        {
            bail!("节点只能监听本机反向代理地址，并且只连接固定回环TS地址和非零端口");
        }
        if let Some(main) = &self.main_url {
            if !self.public_url.is_empty() {
                bail!("反向节点只填写main_url，删除旧public_url");
            }
            endpoint(main, self.allow_insecure_localhost)?;
        } else {
            endpoint(&self.public_url, self.allow_insecure_localhost)?;
        }
        self.runtime_config().validate()?;
        self.image_limits.validate()?;
        Ok(())
    }
    fn runtime_config(&self) -> Config {
        Config {
            bind: self.bind.to_string(),
            public_url: self.main_url.as_ref().unwrap_or(&self.public_url).clone(),
            database: ":memory:".into(),
            master_key_file: String::new(),
            web_dir: String::new(),
            max_connections: self.max_connections,
            trusted_proxy: vec![],
            allow_insecure_localhost: self.allow_insecure_localhost,
            servers: vec![],
            voice_nodes: vec![],
            smtp: None,
            rtc: self.rtc.clone(),
        }
    }
}
struct Node {
    app: Arc<App>,
    auth: Authentication,
    target: SocketAddr,
    file_port: u16,
}
fn response_limits() -> WebSocketConfig {
    // A bounded 192 KiB channel PNG expands to 256 KiB before JSON metadata.
    // Incoming browser commands keep their separate 256 KiB limit.
    WebSocketConfig::default()
        .max_message_size(Some(1024 * 1024))
        .max_frame_size(Some(1024 * 1024))
}
pub fn check(path: &str) -> Result<()> {
    let config = NodeConfig::load(path)?;
    key_from_text(&Zeroizing::new(std::fs::read_to_string(&config.key_file)?))?;
    Ok(())
}
pub async fn serve(config: NodeConfig) -> Result<()> {
    config.validate()?;
    let key = key_from_text(&Zeroizing::new(std::fs::read_to_string(&config.key_file)?))?;
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let app = App::node_runtime(config.runtime_config())?;
    let outbound_stop = CancellationToken::new();
    let outbound = config.main_url.as_ref().map(|_| {
        let config = config.clone();
        let stop = outbound_stop.clone();
        tokio::spawn(async move { reverse::agent(&config, stop).await })
    });
    app.runtime.write().unwrap().settings.image_limits = config.image_limits;
    let node = Arc::new(Node {
        app: app.clone(),
        target: config.target,
        file_port: config.file_transfer_port,
        auth: Authentication {
            id: config.id,
            key,
            seen: Mutex::new(HashMap::new()),
        },
    });
    let router = router(node);
    let shutdown = app.clone();
    tracing::info!("WebTS同机语音节点已启动，仅接受配对站点授权");
    let stop_agent = outbound_stop.clone();
    let result = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            crate::node::stop_signal().await;
            stop_agent.cancel();
            shutdown.connections.shutdown();
        })
        .await;
    outbound_stop.cancel();
    if let Some(task) = outbound {
        let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
    }
    if !app.connections.wait_closed().await {
        tracing::warn!("节点关闭等待超时，进程退出将回收剩余套接字");
    }
    result?;
    Ok(())
}
fn router(node: Arc<Node>) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async {
                Json(json!({"role":"voice-node","version":env!("CARGO_PKG_VERSION")}))
            }),
        )
        .route("/node/connect", get(upgrade))
        .route("/node/rtc", get(ice))
        .route("/node/rtc-stream", get(ice_stream))
        .with_state(node)
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
}
async fn stop_signal() {
    #[cfg(unix)]
    if let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! { _=term.recv()=>{}, _=tokio::signal::ctrl_c()=>{} }
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}
fn authenticate(node: &Node, headers: &HeaderMap, path: &str) -> Api<()> {
    node.auth
        .verify(headers, path, crate::db::now())
        .map_err(|_| Error(StatusCode::UNAUTHORIZED, "节点授权无效"))
}
async fn upgrade(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Api<Response> {
    authenticate(&node, &headers, "/node/connect")?;
    let permit = node
        .app
        .sockets
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "节点连接已满"))?;
    Ok(ws
        .max_message_size(256 * 1024)
        .max_frame_size(256 * 1024)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            crate::gateway::run_node(node.app.clone(), node.target, node.file_port, socket).await;
        }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IceOwner {
    owner: i64,
}
async fn ice(
    State(node): State<Arc<Node>>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
    Query(query): Query<IceOwner>,
) -> Api<Json<Value>> {
    authenticate(
        &node,
        &headers,
        uri.path_and_query().map(|u| u.as_str()).unwrap_or(""),
    )?;
    if query.owner <= 0 {
        return Err(Error::bad("账号无效"));
    }
    Ok(Json(
        json!({"iceServers":crate::media::ice_servers(&node.app,query.owner)?}),
    ))
}

async fn ice_stream(
    State(node): State<Arc<Node>>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
    Query(query): Query<IceOwner>,
    ws: WebSocketUpgrade,
) -> Api<Response> {
    authenticate(
        &node,
        &headers,
        uri.path_and_query().map(|u| u.as_str()).unwrap_or(""),
    )?;
    if query.owner <= 0 {
        return Err(Error::bad("账号无效"));
    }
    let value = json!({"iceServers":crate::media::ice_servers(&node.app, query.owner)?});
    Ok(ws
        .max_message_size(8192)
        .max_frame_size(8192)
        .on_upgrade(move |mut socket| async move {
            let _ = tokio::time::timeout(
                Duration::from_secs(2),
                socket.send(axum::extract::ws::Message::Text(value.to_string().into())),
            )
            .await;
            let _ = tokio::time::timeout(Duration::from_secs(2), socket.close()).await;
        }))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Grant {
    pub owner: i64,
    pub session: String,
    pub expires: i64,
    pub identity: String,
    pub uid: String,
    pub page: String,
    pub name: String,
    pub password: String,
}
impl Drop for Grant {
    fn drop(&mut self) {
        self.identity.zeroize();
        self.password.zeroize();
    }
}
impl RemoteNode {
    async fn key(&self) -> Result<Zeroizing<[u8; 32]>> {
        key_from_text(&Zeroizing::new(
            tokio::fs::read_to_string(&self.key_file).await?,
        ))
    }
    pub(crate) async fn ice(&self, app: &Arc<App>, owner: i64) -> Result<Value> {
        if self.reverse {
            let mut socket = self
                .socket(
                    app,
                    &format!("/node/rtc-stream?owner={owner}"),
                    WebSocketConfig::default()
                        .max_message_size(Some(8192))
                        .max_frame_size(Some(8192)),
                )
                .await?;
            let value = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await?
                .context("节点ICE已关闭")??;
            let value: Value = serde_json::from_str(value.to_text()?)?;
            if !value["iceServers"].as_array().is_some_and(|s| s.len() <= 8) {
                bail!("节点ICE配置无效");
            }
            return Ok(value);
        }
        let _ = rustls::crypto::ring::default_provider().install_default();
        let path = format!("/node/rtc?owner={owner}");
        let url = format!("{}{path}", self.url.trim_end_matches('/'));
        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()?
            .get(url)
            .headers(signed_headers(self.key().await?.as_ref(), &self.id, &path)?)
            .send()
            .await?
            .error_for_status()?;
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if bytes.len() + chunk.len() > 8192 {
                bail!("节点ICE配置过大");
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes)?;
        if !value["iceServers"].as_array().is_some_and(|s| s.len() <= 8) {
            bail!("节点ICE配置无效");
        }
        Ok(value)
    }
    async fn socket(
        &self,
        app: &Arc<App>,
        path: &str,
        config: WebSocketConfig,
    ) -> Result<reverse::Peer> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut url = if self.reverse {
            url::Url::parse("ws://localhost")?
        } else {
            url::Url::parse(&self.url)?
        };
        if !self.reverse {
            url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
                .map_err(|_| anyhow::anyhow!("节点URL无效"))?;
        }
        let (route, query) = path
            .split_once('?')
            .map_or((path, None), |(p, q)| (p, Some(q)));
        url.set_path(route);
        url.set_query(query);
        let mut request = url.as_str().into_client_request()?;
        request
            .headers_mut()
            .extend(signed_headers(self.key().await?.as_ref(), &self.id, path)?);
        if self.reverse {
            let stream = app.node_links.open(&self.id).await?;
            let (socket, _) = tokio::time::timeout(
                Duration::from_secs(8),
                tokio_tungstenite::client_async_with_config(request, stream, Some(config)),
            )
            .await??;
            Ok(Box::new(socket))
        } else {
            let (socket, _) = tokio::time::timeout(
                Duration::from_secs(8),
                tokio_tungstenite::connect_async_with_config(request, Some(config), true),
            )
            .await??;
            Ok(Box::new(socket))
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn relay(
        &self,
        app: &Arc<App>,
        headers: &HeaderMap,
        grant: Grant,
        input: &mut crate::gateway::BrowserInput,
        tx: &mpsc::Sender<Value>,
        cancel: &CancellationToken,
        done: &CancellationToken,
        connection: &str,
    ) -> Result<()> {
        let mut remote = self.socket(app, "/node/connect", response_limits()).await?;
        app.session_async(headers)
            .await
            .map_err(|_| anyhow::anyhow!("登录已失效"))?;
        if cancel.is_cancelled() || done.is_cancelled() {
            bail!("连接已撤销");
        }
        let text = Zeroizing::new(serde_json::to_string(&grant)?);
        drop(grant);
        tokio::time::timeout(
            Duration::from_secs(2),
            remote.send(Message::Text(text.to_string().into())),
        )
        .await??;
        let result=async {
            let session=crate::gateway::session_check(app,headers);tokio::pin!(session);
            let mut timer=tokio::time::interval(Duration::from_secs(10)); timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut browser_seen=Instant::now();let mut node_seen=Instant::now();let mut rate=(Instant::now(),0u32);
            loop { tokio::select! {
                _=cancel.cancelled()=>bail!("连接已撤销"), _=done.cancelled()=>bail!("网页连接已关闭"), result=&mut session=>return result,
                _=timer.tick()=>{if browser_seen.elapsed()>=Duration::from_secs(45)||node_seen.elapsed()>=Duration::from_secs(45){bail!("节点或网页存活检测超时");}tokio::time::timeout(Duration::from_secs(2),remote.send(Message::Ping(vec![].into()))).await??;},
                message=input.next()=>{
                    let message=message.context("网页连接已关闭")??;browser_seen=Instant::now();
                    match message {
                        axum::extract::ws::Message::Text(text)=>{
                            if rate.0.elapsed()>=Duration::from_secs(1){rate=(Instant::now(),0);}rate.1+=1;if rate.1>40{bail!("网页操作过于频繁");}
                            tokio::time::timeout(Duration::from_secs(2),remote.send(Message::Text(text.to_string().into()))).await??;
                        },
                        axum::extract::ws::Message::Close(_)=>break,
                        axum::extract::ws::Message::Binary(_)=>bail!("不接受二进制中转"), _=>{}
                    }
                },
                message=remote.next()=>{
                    let message=message.context("语音节点连接已关闭")??;node_seen=Instant::now();
                    match message {
                        Message::Text(text)=>{let mut value:Value=serde_json::from_str(&text)?;if !value.is_object()||!value["type"].as_str().is_some_and(|s|!s.is_empty()&&s.len()<=64){bail!("节点事件格式无效");}
                            if value["type"]=="state" {app.connections.update_location(connection,&mut value);}value["voice_route"]=json!({"kind":"node","node":self.id});tx.try_send(value).context("网页接收过慢")?;},
                        Message::Ping(data)=>{tokio::time::timeout(Duration::from_secs(2),remote.send(Message::Pong(data))).await??;},
                        Message::Close(_)=>bail!("语音节点连接已关闭"),Message::Binary(_)=>bail!("节点禁止二进制中转"),_=>{}
                    }
                }
            }}
            Ok(())
        }.await;
        let _ = tokio::time::timeout(Duration::from_secs(2), remote.close()).await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    fn auth() -> Authentication {
        Authentication {
            id: "fixture".into(),
            key: Zeroizing::new([41; 32]),
            seen: Mutex::new(HashMap::new()),
        }
    }
    fn config() -> NodeConfig {
        NodeConfig {
            id: "fixture".into(),
            bind: "127.0.0.1:18098".parse().unwrap(),
            public_url: "http://127.0.0.1:18098".into(),
            main_url: None,
            target: "127.0.0.1:19987".parse().unwrap(),
            file_transfer_port: 30033,
            key_file: String::new(),
            max_connections: 2,
            allow_insecure_localhost: true,
            rtc: Rtc {
                udp_min: 41000,
                udp_max: 41001,
                ..Default::default()
            },
            image_limits: Default::default(),
        }
    }
    fn grant() -> Grant {
        let identity = tsclientlib::Identity::create();
        Grant {
            owner: 1,
            session: "ab".repeat(32),
            expires: crate::db::now() + 60,
            identity: crate::identity::canonical(&identity).to_string(),
            uid: crate::identity::uid(&identity),
            page: "fixture".into(),
            name: "Node fixture".into(),
            password: String::new(),
        }
    }
    #[test]
    fn authentication_is_bound_to_node_path_time_nonce_and_machine_origin() {
        let auth = auth();
        let now = crate::db::now();
        let headers = signed_headers(&[41; 32], "fixture", "/node/rtc?owner=1").unwrap();
        assert!(auth.verify(&headers, "/node/rtc?owner=2", now).is_err());
        assert!(auth.verify(&headers, "/node/connect", now).is_err());
        assert!(
            auth.verify(&headers, "/node/rtc?owner=1", now + 31)
                .is_err()
        );
        let mut bad = headers.clone();
        bad.insert("origin", HeaderValue::from_static("https://evil.example"));
        assert!(auth.verify(&bad, "/node/rtc?owner=1", now).is_err());
        let mut bad = headers.clone();
        bad.insert("cookie", HeaderValue::from_static("webts_dev=anything"));
        assert!(auth.verify(&bad, "/node/rtc?owner=1", now).is_err());
        assert!(
            auth.verify(
                &signed_headers(&[42; 32], "fixture", "/node/rtc?owner=1").unwrap(),
                "/node/rtc?owner=1",
                now
            )
            .is_err()
        );
        assert!(auth.verify(&headers, "/node/rtc?owner=1", now).is_ok());
        assert!(auth.verify(&headers, "/node/rtc?owner=1", now).is_err());
    }
    #[test]
    fn configuration_requires_fixed_local_target_tls_and_bounded_capacity() {
        let mut c = config();
        assert!(c.validate().is_ok());
        for address in [
            "10.0.0.1:9987",
            "1.1.1.1:9987",
            "0.0.0.0:9987",
            "127.0.0.1:0",
        ] {
            c.target = address.parse().unwrap();
            assert!(c.validate().is_err());
        }
        let mut c = config();
        c.bind = "0.0.0.0:8080".parse().unwrap();
        assert!(c.validate().is_err());
        let mut c = config();
        c.max_connections = 3;
        assert!(c.validate().is_err());
        let mut c = config();
        c.file_transfer_port = 0;
        assert!(c.validate().is_err());
        let example: NodeConfig = toml::from_str(include_str!("../../node.example.toml")).unwrap();
        assert!(example.validate().is_ok());
        for url in [
            "http://example.com",
            "https://user:pass@example.com",
            "https://example.com/path",
            "https://example.com?secret=x",
            "https://example.com/#x",
        ] {
            assert!(endpoint(url, false).is_err());
        }
        assert!(endpoint("https://node.example", false).is_ok());
        assert!(endpoint("http://127.0.0.1:8080", false).is_err());
        assert!(endpoint("http://127.0.0.1:8080", true).is_ok());
        assert!(key_from_text(&hex::encode([0; 32])).is_err());
        assert!(key_from_text("short").is_err());
        let mut value = serde_json::to_value(grant()).unwrap();
        value["target"] = json!("169.254.169.254:80");
        assert!(serde_json::from_value::<Grant>(value).is_err());
    }
    #[tokio::test]
    async fn node_response_codec_accepts_bounded_images_but_rejects_oversized_frames() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let image = json!({"type":"ts_image","id":"fixture","data":format!("data:image/png;base64,{}",STANDARD.encode(vec![42u8;192*1024]))}).to_string();
        assert!(image.len() > 256 * 1024);
        for (text, accepted) in [(image, true), ("x".repeat(1024 * 1024 + 1), false)] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let expected = text.clone();
            let peer = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                socket.send(Message::Text(text.into())).await.ok();
            });
            let (mut socket, _) = tokio_tungstenite::connect_async_with_config(
                format!("ws://{address}"),
                Some(response_limits()),
                false,
            )
            .await
            .unwrap();
            let result = tokio::time::timeout(Duration::from_secs(3), socket.next())
                .await
                .unwrap()
                .unwrap();
            if accepted {
                assert_eq!(result.unwrap().to_text().unwrap(), expected);
            } else {
                assert!(matches!(
                    result,
                    Err(tokio_tungstenite::tungstenite::Error::Capacity(_))
                ));
            }
            peer.await.unwrap();
        }
    }
    #[tokio::test]
    async fn node_has_no_site_routes_and_ice_requires_single_use_machine_authentication() {
        let app = App::node_runtime(config().runtime_config()).unwrap();
        let node = Arc::new(Node {
            app: app.clone(),
            auth: auth(),
            target: config().target,
            file_port: 30033,
        });
        let router = router(node);
        assert!(app.db.identities(1).unwrap().is_empty());
        let no = router
            .clone()
            .oneshot(
                Request::get("/node/rtc?owner=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(no.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            router
                .clone()
                .oneshot(Request::get("/api/identities").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        let headers = signed_headers(&[41; 32], "fixture", "/node/rtc?owner=1").unwrap();
        let request = || {
            let mut r = Request::get("/node/rtc?owner=1")
                .body(Body::empty())
                .unwrap();
            *r.headers_mut() = headers.clone();
            r
        };
        let good = router.clone().oneshot(request()).await.unwrap();
        assert_eq!(good.status(), StatusCode::OK);
        assert_eq!(good.headers()["cache-control"], "no-store");
        let body = to_bytes(good.into_body(), 8192).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["iceServers"],
            json!([])
        );
        assert_eq!(
            router.oneshot(request()).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn authenticated_node_dials_only_bound_ts_and_close_during_handshake_releases_lease() {
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let app = App::node_runtime(config().runtime_config()).unwrap();
        let node = Arc::new(Node {
            app: app.clone(),
            auth: auth(),
            target: udp.local_addr().unwrap(),
            file_port: 30033,
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let stopped = CancellationToken::new();
        let signal = stopped.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, router(node))
                .with_graceful_shutdown(signal.cancelled_owned())
                .await
                .unwrap();
        });
        let mut request = format!("ws://{address}/node/connect")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .extend(signed_headers(&[41; 32], "fixture", "/node/connect").unwrap());
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let mut malformed = serde_json::to_value(grant()).unwrap();
        malformed["target"] = json!("1.1.1.1:9987");
        socket
            .send(Message::Text(malformed.to_string().into()))
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(150), udp.recv_from(&mut [0u8; 2048]))
                .await
                .is_err()
        );
        socket.close(None).await.ok();
        let mut request = format!("ws://{address}/node/connect")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .extend(signed_headers(&[41; 32], "fixture", "/node/connect").unwrap());
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        socket
            .send(Message::Text(
                serde_json::to_string(&grant()).unwrap().into(),
            ))
            .await
            .unwrap();
        let (size, peer) =
            tokio::time::timeout(Duration::from_secs(5), udp.recv_from(&mut [0u8; 2048]))
                .await
                .unwrap()
                .unwrap();
        assert!(size > 0);
        assert!(peer.ip().is_loopback());
        assert_eq!(app.connections.total(), 1);
        socket.close(None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while app.connections.total() != 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(app.db.identities(1).unwrap().is_empty());
        stopped.cancel();
        task.await.unwrap();
    }
    #[tokio::test]
    async fn site_routes_authorized_identity_to_node_and_revocation_closes_both_leases() {
        let directory = tempfile::tempdir().unwrap();
        let key = directory.path().join("node.key");
        std::fs::write(&key, hex::encode([41; 32])).unwrap();
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local = App::node_runtime(config().runtime_config()).unwrap();
        let node = Arc::new(Node {
            app: local.clone(),
            auth: auth(),
            target: udp.local_addr().unwrap(),
            file_port: 30033,
        });
        let node_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let node_address = node_listener.local_addr().unwrap();
        let stop_node = CancellationToken::new();
        let signal = stop_node.clone();
        let node_task = tokio::spawn(async move {
            axum::serve(node_listener, router(node))
                .with_graceful_shutdown(signal.cancelled_owned())
                .await
                .unwrap();
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut cfg = config().runtime_config();
        cfg.public_url = format!(
            "http://127.0.0.1:{address_port}",
            address_port = address.port()
        );
        cfg.servers = vec![crate::config::Server {
            id: "ts-local".into(),
            name: "local fixture".into(),
            address: "1.1.1.1:9987".into(),
        }];
        cfg.voice_nodes = vec![RemoteNode {
            id: "fixture".into(),
            server: "ts-local".into(),
            url: format!("http://{node_address}"),
            reverse: false,
            key_file: key.to_string_lossy().into_owned(),
        }];
        assert!(cfg.validate().is_ok());
        let site = App::node_runtime(cfg).unwrap();
        let (owner, _, verify) = site
            .db
            .register("node-owner@example.invalid", "fixture-hash")
            .unwrap();
        site.db
            .consume_email_token(&verify, "verify", None)
            .unwrap();
        let (other, _, verify) = site
            .db
            .register("node-other@example.invalid", "fixture-hash")
            .unwrap();
        site.db
            .consume_email_token(&verify, "verify", None)
            .unwrap();
        let token = site
            .db
            .create_session(owner, false, "fixture-hash")
            .unwrap();
        let other_token = site
            .db
            .create_session(other, false, "fixture-hash")
            .unwrap();
        let identity = site
            .db
            .add_identity(
                owner,
                "fixture",
                &tsclientlib::Identity::create(),
                &site.vault,
            )
            .unwrap();
        let route = crate::app::router(site.clone());
        let rtc = route
            .clone()
            .oneshot(
                Request::get("/api/rtc?server=ts-local")
                    .header("cookie", format!("webts_dev={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rtc.status(), StatusCode::OK);
        let stop_site = CancellationToken::new();
        let signal = stop_site.clone();
        let site_task = tokio::spawn(async move {
            axum::serve(listener, route)
                .with_graceful_shutdown(signal.cancelled_owned())
                .await
                .unwrap();
        });
        let ws_request = |credential: &str| {
            let mut request = format!("ws://{address}/api/connect")
                .into_client_request()
                .unwrap();
            request.headers_mut().insert(
                "origin",
                HeaderValue::from_str(&site.config.origin()).unwrap(),
            );
            request.headers_mut().insert(
                "cookie",
                HeaderValue::from_str(&format!("webts_dev={credential}")).unwrap(),
            );
            request
        };
        let (mut rejected, _) = tokio_tungstenite::connect_async(ws_request(&other_token))
            .await
            .unwrap();
        let request = json!({"server":"ts-local","identity":identity,"page":"fixture","name":"Fixture owner"});
        rejected
            .send(Message::Text(request.to_string().into()))
            .await
            .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(3), rejected.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(response.to_text().unwrap().contains("error"));
        assert_eq!(local.connections.total(), 0);
        rejected.close(None).await.ok();
        let (mut browser, _) = tokio_tungstenite::connect_async(ws_request(&token))
            .await
            .unwrap();
        browser
            .send(Message::Text(request.to_string().into()))
            .await
            .unwrap();
        browser
            .send(Message::Text(
                json!({"type":"mute","muted":false,"deafened":false})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), udp.recv_from(&mut [0u8; 2048]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(site.connections.total(), 1);
        assert_eq!(local.connections.total(), 1);
        site.db
            .revoke(owner, Some(&crate::db::digest(&token)))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while site.connections.total() != 0 || local.connections.total() != 0 {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        assert!(local.db.identities(owner).unwrap().is_empty());
        browser.close(None).await.ok();
        stop_site.cancel();
        stop_node.cancel();
        site_task.await.unwrap();
        node_task.await.unwrap();
    }
}
