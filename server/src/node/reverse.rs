//! Outbound WSS carries bounded control streams only; media never enters this tunnel.
use super::*;
use axum::extract::ws::{Message as AxumMessage, WebSocket};
use futures::{Sink, Stream};
use std::collections::VecDeque;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream},
    sync::{Notify, Semaphore, oneshot},
    task::JoinSet,
};
use tokio_tungstenite::tungstenite::{Error as WsError, Result as WsResult};

const PATH: &str = "/api/voice-nodes/tunnel";
const CHUNK: usize = 16 * 1024;
const IDLE: usize = 2;

pub(crate) trait Wire:
    Stream<Item = WsResult<Message>> + Sink<Message, Error = WsError> + Unpin + Send
{
}
impl<T> Wire for T where
    T: Stream<Item = WsResult<Message>> + Sink<Message, Error = WsError> + Unpin + Send
{
}
pub(crate) type Peer = Box<dyn Wire>;

// SinkMapErr consumes its mapper after one error; repeated close/error paths
// must remain safe. Map each operation directly instead of using that adapter.
struct ServerPeer(WebSocket);
impl Stream for ServerPeer {
    type Item = WsResult<Message>;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.get_mut().0)
            .poll_next(cx)
            .map(|item| {
                item.map(|r| {
                    r.map(|m| match m {
                        AxumMessage::Text(t) => Message::Text(t.to_string().into()),
                        AxumMessage::Binary(b) => Message::Binary(b),
                        AxumMessage::Ping(b) => Message::Ping(b),
                        AxumMessage::Pong(b) => Message::Pong(b),
                        AxumMessage::Close(_) => Message::Close(None),
                    })
                    .map_err(|_| WsError::ConnectionClosed)
                })
            })
    }
}
impl Sink<Message> for ServerPeer {
    type Error = WsError;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<WsResult<()>> {
        std::pin::Pin::new(&mut self.get_mut().0)
            .poll_ready(cx)
            .map_err(|_| WsError::ConnectionClosed)
    }
    fn start_send(self: std::pin::Pin<&mut Self>, message: Message) -> WsResult<()> {
        let message = match message {
            Message::Text(t) => AxumMessage::Text(t.to_string().into()),
            Message::Binary(b) => AxumMessage::Binary(b),
            Message::Ping(b) => AxumMessage::Ping(b),
            Message::Pong(b) => AxumMessage::Pong(b),
            Message::Close(_) => AxumMessage::Close(None),
            _ => return Err(WsError::ConnectionClosed),
        };
        std::pin::Pin::new(&mut self.get_mut().0)
            .start_send(message)
            .map_err(|_| WsError::ConnectionClosed)
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<WsResult<()>> {
        std::pin::Pin::new(&mut self.get_mut().0)
            .poll_flush(cx)
            .map_err(|_| WsError::ConnectionClosed)
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<WsResult<()>> {
        std::pin::Pin::new(&mut self.get_mut().0)
            .poll_close(cx)
            .map_err(|_| WsError::ConnectionClosed)
    }
}
fn server_peer(socket: WebSocket) -> Peer {
    Box::new(ServerPeer(socket))
}

struct Pool {
    auth: Authentication,
    idle: Mutex<VecDeque<oneshot::Sender<DuplexStream>>>,
    available: Notify,
    handshakes: Arc<Semaphore>,
}
pub(crate) struct Links {
    nodes: HashMap<String, Arc<Pool>>,
    budget: Arc<Semaphore>,
    stop: CancellationToken,
    pairs: Mutex<HashMap<String, Pairing>>,
}
struct Pairing {
    node: String,
    owner: i64,
    session: String,
    expires: i64,
}
impl Links {
    pub(crate) fn new(config: &Config) -> Result<Self> {
        let mut nodes = HashMap::new();
        for node in config.voice_nodes.iter().filter(|n| n.reverse) {
            nodes.insert(
                node.id.clone(),
                Arc::new(Pool {
                    auth: Authentication {
                        id: node.id.clone(),
                        key: key_from_text(&Zeroizing::new(std::fs::read_to_string(
                            &node.key_file,
                        )?))?,
                        seen: Mutex::new(HashMap::new()),
                    },
                    idle: Mutex::new(VecDeque::new()),
                    available: Notify::new(),
                    handshakes: Arc::new(Semaphore::new(IDLE)),
                }),
            );
        }
        Ok(Self {
            budget: Arc::new(Semaphore::new(
                config.max_connections * 2 + nodes.len() * IDLE,
            )),
            nodes,
            stop: CancellationToken::new(),
            pairs: Mutex::new(HashMap::new()),
        })
    }
    pub(crate) fn shutdown(&self) {
        self.stop.cancel();
    }
    pub(crate) async fn open(&self, id: &str) -> Result<DuplexStream> {
        let pool = self.nodes.get(id).context("未配置此反向节点")?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if self.stop.is_cancelled() {
                    bail!("主站正在退出");
                }
                let notified = pool.available.notified();
                tokio::pin!(notified);
                // Register before inspecting the queue: concurrent openers must
                // not collapse two arrivals into one stored notification.
                notified.as_mut().enable();
                let sender = {
                    let mut idle = pool.idle.lock().unwrap();
                    idle.retain(|s| !s.is_closed());
                    idle.pop_front()
                };
                if let Some(sender) = sender {
                    let (client, server) = tokio::io::duplex(32 * 1024);
                    if sender.send(server).is_ok() {
                        return Ok(client);
                    }
                } else {
                    tokio::select! {_=notified=>{},_=self.stop.cancelled()=>bail!("主站正在退出")}
                }
            }
        })
        .await
        .context("反向节点尚未连接或繁忙")?
    }
}

pub(crate) async fn accept(
    State(app): State<Arc<App>>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Api<Response> {
    let id = headers
        .get("x-webts-node-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let pool = app
        .node_links
        .nodes
        .get(id)
        .cloned()
        .ok_or(Error(StatusCode::UNAUTHORIZED, "节点授权无效"))?;
    pool.auth
        .verify(
            &headers,
            uri.path_and_query().map(|p| p.as_str()).unwrap_or(""),
            crate::db::now(),
        )
        .map_err(|_| Error(StatusCode::UNAUTHORIZED, "节点授权无效"))?;
    if app.node_links.stop.is_cancelled() {
        return Err(Error(StatusCode::SERVICE_UNAVAILABLE, "主站正在退出"));
    }
    let permit = app
        .node_links
        .budget
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "节点通道已满"))?;
    let stop = app.node_links.stop.clone();
    let handshake = pool
        .handshakes
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "节点握手繁忙"))?;
    Ok(ws.max_message_size(CHUNK).max_frame_size(CHUNK).on_upgrade(move |socket|async move {
        let _permit=permit;
        let mut socket=server_peer(socket);
        let result=async {
            let hello=tokio::time::timeout(Duration::from_secs(5),socket.next()).await?.context("节点未就绪")??;
            if hello!=Message::Text("ready-v1".into()) {bail!("节点协议版本无效");}
            drop(handshake);
            let (sender,mut receiver)=oneshot::channel();
            {
                let mut idle=pool.idle.lock().unwrap();idle.retain(|s|!s.is_closed());
                if idle.len()>=IDLE {bail!("备用通道已满");}
                idle.push_back(sender);
            }
            pool.available.notify_one();
            send(&mut socket,Message::Text("ready-v1".into())).await?;
            let mut timer=tokio::time::interval(Duration::from_secs(10));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut seen=Instant::now();
            loop {tokio::select! {
                _=stop.cancelled()=>break,
                stream=&mut receiver=>{
                    let stream=stream.context("通道已取消")?;
                    send(&mut socket,Message::Text("open-v1".into())).await?;
                    return bridge(&mut socket,stream,&stop).await;
                },
                _=timer.tick()=>{if seen.elapsed()>Duration::from_secs(45){bail!("节点心跳超时");}send(&mut socket,Message::Ping(Default::default())).await?;},
                message=socket.next()=>{let message=message.context("节点已断开")??;seen=Instant::now();match message {Message::Ping(b)=>send(&mut socket,Message::Pong(b)).await?,Message::Pong(_)=>{},_=>bail!("备用通道消息无效")}}
            }}
            Ok(())
        }.await;
        let _=result;
        let _=tokio::time::timeout(Duration::from_secs(2),socket.close()).await;
    }))
}

pub(crate) async fn status(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a| {
        crate::settings::admin(a, &headers)?;
        let nodes = a
            .config
            .voice_nodes
            .iter()
            .filter(|n| n.reverse)
            .map(|n| {
                let mut idle = a.node_links.nodes[&n.id].idle.lock().unwrap();
                idle.retain(|s| !s.is_closed());
                json!({"id":n.id,"server":n.server,"idle":idle.len()})
            })
            .collect::<Vec<_>>();
        Ok(Json(json!({"nodes":nodes})))
    })
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Issue {
    id: String,
    password: String,
}
pub(crate) async fn issue(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Issue>,
) -> Api<Json<Value>> {
    app.expensive(move|a| {
        let password=Zeroizing::new(std::mem::take(&mut body.password));
        crate::settings::admin(a,&headers)?;
        if password.len()>1024 {return Err(Error::bad("密码过长"));}
        let session=a.reauthenticate(&headers,&password)?;
        if !a.node_links.nodes.contains_key(&body.id) {return Err(Error::bad("未配置此反向节点"));}
        let code=Zeroizing::new(crate::vault::token()?);
        let now=crate::db::now();let expires=(now+300).min(session.expires);
        let mut pairs=a.node_links.pairs.lock().unwrap();
        pairs.retain(|_,p|p.expires>now&&p.node!=body.id);
        pairs.insert(crate::db::digest(&code),Pairing{node:body.id.clone(),owner:session.user.id,session:session.hash,expires});
        Ok(Json(json!({"code":code.as_str(),"node":body.id,"expires":expires,"main_url":a.config.public_url})))
    }).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Redeem {
    code: String,
}
pub(crate) async fn redeem(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Redeem>,
) -> Api<Json<Value>> {
    if headers.contains_key("cookie") || headers.contains_key("origin") {
        return Err(Error(StatusCode::FORBIDDEN, "仅接受节点配对请求"));
    }
    let code = Zeroizing::new(std::mem::take(&mut body.code));
    if code.len() != 64 || !code.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error(StatusCode::UNAUTHORIZED, "配对码无效或过期"));
    }
    app.work(move|a| {
        let now=crate::db::now();
        let pair={let mut pairs=a.node_links.pairs.lock().unwrap();pairs.retain(|_,p|p.expires>now);pairs.remove(&crate::db::digest(&code))}.ok_or(Error(StatusCode::UNAUTHORIZED,"配对码无效或过期"))?;
        let valid:bool=a.db.connection.lock().unwrap().query_row("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id LEFT JOIN session_details d ON d.hash=s.hash WHERE s.hash=?1 AND u.id=?2 AND s.expires>?3 AND u.verified=1 AND u.is_admin=1 AND u.banned_until!=-1 AND u.banned_until<=?3 AND COALESCE(d.public_device,0)=0)",rusqlite::params![pair.session,pair.owner,now],|r|r.get(0)).map_err(anyhow::Error::from)?;
        if !valid {return Err(Error(StatusCode::UNAUTHORIZED,"配对授权已撤销"));}
        let pool=a.node_links.nodes.get(&pair.node).ok_or(Error(StatusCode::UNAUTHORIZED,"节点已移除"))?;
        Ok(Json(json!({"id":pair.node,"main_url":a.config.public_url,"key":hex::encode(pool.auth.key.as_ref())})))
    }).await
}

async fn send(socket: &mut Peer, message: Message) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message)).await??;
    Ok(())
}
async fn bridge<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut Peer,
    mut io: S,
    stop: &CancellationToken,
) -> Result<()> {
    let mut bytes = vec![0u8; CHUNK];
    let mut seen = Instant::now();
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _=stop.cancelled()=>break,
            read=io.read(&mut bytes)=>{let size=read?;if size==0{break;}send(socket,Message::Binary(bytes[..size].to_vec().into())).await?;},
            message=socket.next()=>{let message=message.context("反向通道已关闭")??;seen=Instant::now();match message {
                Message::Binary(b) if b.len()<=CHUNK=>{tokio::time::timeout(Duration::from_secs(5),io.write_all(&b)).await??;},
                Message::Ping(b)=>send(socket,Message::Pong(b)).await?,Message::Pong(_)=>{},Message::Close(_)=>break,_=>bail!("反向通道帧无效")
            }},
            _=timer.tick()=>{if seen.elapsed()>Duration::from_secs(45){bail!("反向通道心跳超时");}send(socket,Message::Ping(Default::default())).await?;}
        }
    }
    // Finish the WebSocket close exchange before dropping TCP. Unread control
    // frames can otherwise turn a short response into a TCP reset and lose it.
    let _ = tokio::time::timeout(Duration::from_secs(2), socket.close()).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(Ok(message)) = socket.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), io.shutdown()).await;
    Ok(())
}

pub(crate) async fn agent(config: &NodeConfig, stop: CancellationToken) -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut url = endpoint(
        config.main_url.as_ref().context("缺少main_url")?,
        config.allow_insecure_localhost,
    )?;
    url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
        .map_err(|_| anyhow::anyhow!("主站地址无效"))?;
    url.set_path(PATH);
    let key = Arc::new(key_from_text(&Zeroizing::new(
        tokio::fs::read_to_string(&config.key_file).await?,
    ))?);
    let idle = Arc::new(Semaphore::new(IDLE));
    let wake = Arc::new(Notify::new());
    let mut tasks = JoinSet::new();
    loop {
        while tasks.len() < config.max_connections + IDLE {
            let Ok(permit) = idle.clone().try_acquire_owned() else {
                break;
            };
            let (url, id, key, wake, stop, target) = (
                url.to_string(),
                config.id.clone(),
                key.clone(),
                wake.clone(),
                stop.clone(),
                config.bind,
            );
            tasks.spawn(async move {
                let mut permit=Some(permit);let mut backoff=1u64;
                loop {
                    let attempt=async {
                        let mut request=url.as_str().into_client_request()?;
                        request.headers_mut().extend(signed_headers(key.as_ref().as_ref(),&id,PATH)?);
                        let config=WebSocketConfig::default().max_message_size(Some(CHUNK)).max_frame_size(Some(CHUNK));
                        let (socket,_)=tokio::time::timeout(Duration::from_secs(8),tokio_tungstenite::connect_async_with_config(request,Some(config),true)).await??;
                        let mut socket:Peer=Box::new(socket);
                        send(&mut socket,Message::Text("ready-v1".into())).await?;
                        let hello=tokio::time::timeout(Duration::from_secs(5),socket.next()).await?.context("主站未就绪")??;
                        if hello!=Message::Text("ready-v1".into()){bail!("主站协议版本无效");}
                        tracing::info!("节点反向控制通道已就绪");
                        backoff=1;
                        let mut seen=Instant::now();let mut timer=tokio::time::interval(Duration::from_secs(10));timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        loop {tokio::select! {
                            _=stop.cancelled()=>return Ok::<_,anyhow::Error>(()),
                            _=timer.tick()=>{if seen.elapsed()>Duration::from_secs(45){bail!("主站心跳超时");}send(&mut socket,Message::Ping(Default::default())).await?;},
                            message=socket.next()=>{let message=message.context("主站已关闭")??;seen=Instant::now();match message {
                                Message::Text(t) if t=="open-v1"=>{
                                    drop(permit.take());wake.notify_one();
                                    let stream=tokio::time::timeout(Duration::from_secs(3),tokio::net::TcpStream::connect(target)).await??;
                                    return bridge(&mut socket,stream,&stop).await;
                                },
                                Message::Ping(b)=>send(&mut socket,Message::Pong(b)).await?,Message::Pong(_)=>{},_=>bail!("主站控制消息无效")
                            }}
                        }}
                    };
                    let result=tokio::select! {_=stop.cancelled()=>return,result=attempt=>result};
                    if permit.is_none() {return;}
                    if result.is_err(){tracing::warn!(retry_seconds=backoff,"节点主站通道中断，稍后重试；未记录凭据");}
                    tokio::select! {_=stop.cancelled()=>return,_=tokio::time::sleep(Duration::from_secs(backoff))=>{}}
                    backoff=(backoff*2).min(30);
                }
            });
        }
        tokio::select! {
            _=stop.cancelled()=>break,
            _=wake.notified()=>{},
            _=tasks.join_next(),if !tasks.is_empty()=>{},
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    struct Fixture {
        site: Arc<App>,
        node: NodeConfig,
        _directory: tempfile::TempDir,
    }
    fn fixture(main: &str, local: std::net::SocketAddr, target: std::net::SocketAddr) -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let key = directory.path().join("node.key");
        std::fs::write(&key, hex::encode([41; 32])).unwrap();
        let mut config: Config =
            toml::from_str(include_str!("../../../config.example.toml")).unwrap();
        config.smtp = None;
        config.public_url = main.into();
        config.allow_insecure_localhost = true;
        config.max_connections = 2;
        config.servers = vec![crate::config::Server {
            id: "ts-local".into(),
            name: "Fixture".into(),
            address: "1.1.1.1:9987".into(),
        }];
        config.voice_nodes = vec![RemoteNode {
            id: "fixture".into(),
            server: "ts-local".into(),
            url: String::new(),
            reverse: true,
            key_file: key.to_string_lossy().into_owned(),
        }];
        assert!(config.validate().is_ok());
        let site = App::node_runtime(config).unwrap();
        let node = NodeConfig {
            id: "fixture".into(),
            bind: local,
            public_url: String::new(),
            main_url: Some(main.into()),
            target,
            file_transfer_port: 30033,
            key_file: key.to_string_lossy().into_owned(),
            max_connections: 2,
            allow_insecure_localhost: true,
            rtc: Rtc {
                udp_min: 42000,
                udp_max: 42001,
                ..Default::default()
            },
            image_limits: Default::default(),
        };
        assert!(node.validate().is_ok());
        Fixture {
            site,
            node,
            _directory: directory,
        }
    }
    fn account(site: &App, email: &str, admin: bool) -> (i64, String) {
        let hash = crate::password::hash("Fixture-Password-2026").unwrap();
        let (id, _, verify) = site.db.register(email, &hash).unwrap();
        site.db
            .consume_email_token(&verify, "verify", None)
            .unwrap();
        if admin {
            site.db.grant_admin(email).unwrap();
        }
        (id, site.db.create_session(id, false, &hash).unwrap())
    }
    fn request(
        path: &str,
        body: Value,
        cookie: Option<&str>,
        origin: Option<&str>,
    ) -> Request<Body> {
        let mut request = Request::post(path).header("content-type", "application/json");
        if let Some(value) = cookie {
            request = request.header("cookie", format!("webts_dev={value}"));
        }
        if let Some(value) = origin {
            request = request.header("origin", value);
        }
        request.body(Body::from(body.to_string())).unwrap()
    }
    async fn value(response: Response) -> Value {
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap()
    }
    #[tokio::test]
    async fn pairing_requires_reauthentication_is_single_use_and_dies_with_admin_session() {
        let f = fixture(
            "http://127.0.0.1:18098",
            "127.0.0.1:18099".parse().unwrap(),
            "127.0.0.1:19987".parse().unwrap(),
        );
        let (owner, token) = account(&f.site, "node-admin@example.invalid", true);
        let (_, other) = account(&f.site, "node-member@example.invalid", false);
        let route = crate::app::router(f.site.clone());
        let origin = f.site.config.origin();
        let good = json!({"id":"fixture","password":"Fixture-Password-2026"});
        for (cookie, source, body, status) in [
            (
                &other,
                Some(origin.as_str()),
                good.clone(),
                StatusCode::FORBIDDEN,
            ),
            (&token, None, good.clone(), StatusCode::FORBIDDEN),
            (
                &token,
                Some(origin.as_str()),
                json!({"id":"fixture","password":"wrong"}),
                StatusCode::UNAUTHORIZED,
            ),
        ] {
            assert_eq!(
                route
                    .clone()
                    .oneshot(request(
                        "/api/admin/voice-nodes/pair",
                        body,
                        Some(cookie),
                        source
                    ))
                    .await
                    .unwrap()
                    .status(),
                status
            );
        }
        let issued = value(
            route
                .clone()
                .oneshot(request(
                    "/api/admin/voice-nodes/pair",
                    good.clone(),
                    Some(&token),
                    Some(&origin),
                ))
                .await
                .unwrap(),
        )
        .await;
        let code = issued["code"].as_str().unwrap();
        assert_eq!(code.len(), 64);
        let redeem = json!({"code":code});
        assert_eq!(
            route
                .clone()
                .oneshot(request(
                    "/api/voice-nodes/pair",
                    redeem.clone(),
                    Some(&token),
                    Some(&origin)
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let (a, b) = tokio::join!(
            route
                .clone()
                .oneshot(request("/api/voice-nodes/pair", redeem.clone(), None, None)),
            route
                .clone()
                .oneshot(request("/api/voice-nodes/pair", redeem, None, None))
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        assert!(
            (a.status() == StatusCode::OK && b.status() == StatusCode::UNAUTHORIZED)
                || (b.status() == StatusCode::OK && a.status() == StatusCode::UNAUTHORIZED)
        );
        let response = if a.status() == StatusCode::OK {
            value(a).await
        } else {
            value(b).await
        };
        assert_eq!(response["id"], "fixture");
        assert_eq!(response["key"], hex::encode([41; 32]));
        let expired = value(
            route
                .clone()
                .oneshot(request(
                    "/api/admin/voice-nodes/pair",
                    good.clone(),
                    Some(&token),
                    Some(&origin),
                ))
                .await
                .unwrap(),
        )
        .await;
        for pair in f.site.node_links.pairs.lock().unwrap().values_mut() {
            pair.expires = crate::db::now() - 1;
        }
        assert_eq!(
            route
                .clone()
                .oneshot(request(
                    "/api/voice-nodes/pair",
                    json!({"code":expired["code"]}),
                    None,
                    None
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let revoked = value(
            route
                .clone()
                .oneshot(request(
                    "/api/admin/voice-nodes/pair",
                    good,
                    Some(&token),
                    Some(&origin),
                ))
                .await
                .unwrap(),
        )
        .await;
        f.site
            .db
            .revoke(owner, Some(&crate::db::digest(&token)))
            .unwrap();
        assert_eq!(
            route
                .oneshot(request(
                    "/api/voice-nodes/pair",
                    json!({"code":revoked["code"]}),
                    None,
                    None
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn repeated_operations_after_transport_close_do_not_panic() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (done, completed) = oneshot::channel();
        let done = Arc::new(Mutex::new(Some(done)));
        let route = Router::new().route(
            "/",
            axum::routing::get(move |ws: WebSocketUpgrade| {
                let done = done.clone();
                async move {
                    ws.on_upgrade(move |socket| async move {
                        let mut socket = server_peer(socket);
                        let _ = socket.close().await;
                        let _ = socket.send(Message::Ping(Default::default())).await;
                        let _ = socket.send(Message::Ping(Default::default())).await;
                        done.lock().unwrap().take().unwrap().send(()).ok();
                    })
                }
            }),
        );
        let stop = CancellationToken::new();
        let signal = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, route)
                .with_graceful_shutdown(signal.cancelled_owned())
                .await
                .unwrap();
        });
        let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), completed)
                .await
                .unwrap()
                .is_ok()
        );
        client.close(None).await.ok();
        stop.cancel();
        task.await.unwrap();
    }
    #[tokio::test]
    async fn outbound_tunnel_rejects_browser_replay_and_excess_stalled_handshakes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let f = fixture(
            &format!("http://{address}"),
            "127.0.0.1:8095".parse().unwrap(),
            "127.0.0.1:9987".parse().unwrap(),
        );
        let stop = CancellationToken::new();
        let signal = stop.clone();
        let site = f.site.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, crate::app::router(site))
                .with_graceful_shutdown(signal.cancelled_owned())
                .await
                .unwrap();
        });
        let request = || {
            let mut r = format!("ws://{address}{PATH}")
                .into_client_request()
                .unwrap();
            r.headers_mut()
                .extend(signed_headers(&[41; 32], "fixture", PATH).unwrap());
            r
        };
        async fn denied(request: tokio_tungstenite::tungstenite::http::Request<()>, status: u16) {
            match tokio_tungstenite::connect_async(request).await {
                Err(WsError::Http(response)) => assert_eq!(response.status().as_u16(), status),
                _ => panic!("expected rejected machine handshake"),
            }
        }
        denied(
            format!("ws://{address}{PATH}")
                .into_client_request()
                .unwrap(),
            401,
        )
        .await;
        for header in ["origin", "cookie"] {
            let mut r = request();
            r.headers_mut()
                .insert(header, "https://example.invalid".parse().unwrap());
            denied(r, 401).await;
        }
        let replay = request();
        let (mut first, _) = tokio_tungstenite::connect_async(replay.clone())
            .await
            .unwrap();
        denied(replay, 401).await;
        let (mut second, _) = tokio_tungstenite::connect_async(request()).await.unwrap();
        denied(request(), 503).await;
        first.close(None).await.ok();
        second.close(None).await.ok();
        tokio::time::timeout(Duration::from_secs(2), async {
            while f.site.node_links.nodes["fixture"]
                .handshakes
                .available_permits()
                != IDLE
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        f.site.node_shutdown();
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn outbound_node_routes_ice_and_identity_through_local_only_gateway_and_cleans_up() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let node_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let f = fixture(
            &format!("http://{address}"),
            node_listener.local_addr().unwrap(),
            udp.local_addr().unwrap(),
        );
        let site = f.site.clone();
        let (owner, token) = account(&site, "node-owner@example.invalid", false);
        let (_, other) = account(&site, "node-other@example.invalid", false);
        let identity = site
            .db
            .add_identity(
                owner,
                "fixture",
                &tsclientlib::Identity::create(),
                &site.vault,
            )
            .unwrap();
        let local = App::node_runtime(f.node.runtime_config()).unwrap();
        let node = Arc::new(super::super::Node {
            app: local.clone(),
            auth: Authentication {
                id: "fixture".into(),
                key: Zeroizing::new([41; 32]),
                seen: Mutex::new(HashMap::new()),
            },
            target: f.node.target,
            file_port: 30033,
        });
        let stop = CancellationToken::new();
        let signal = stop.clone();
        let main_app = site.clone();
        let main_task = tokio::spawn(async move {
            axum::serve(listener, crate::app::router(main_app))
                .with_graceful_shutdown(signal.cancelled_owned())
                .await
                .unwrap();
        });
        let signal = stop.clone();
        let node_task = tokio::spawn(async move {
            axum::serve(node_listener, super::super::router(node))
                .with_graceful_shutdown(signal.cancelled_owned())
                .await
                .unwrap();
        });
        let agent_stop = CancellationToken::new();
        let signal = agent_stop.clone();
        let config = f.node.clone();
        let agent_task = tokio::spawn(async move {
            agent(&config, signal).await.unwrap();
        });
        let relay = &site.config.voice_nodes[0];
        // Fast local replies used to be truncated on close. Exercise both idle
        // lanes repeatedly, including replenishment while another lane closes.
        for _ in 0..8 {
            let (a, b) = tokio::join!(relay.ice(&site, owner), relay.ice(&site, owner));
            assert_eq!(a.unwrap(), json!({"iceServers":[]}));
            assert_eq!(b.unwrap(), json!({"iceServers":[]}));
        }
        let ws = |token: &str| {
            let mut r = format!("ws://{address}/api/connect")
                .into_client_request()
                .unwrap();
            r.headers_mut()
                .insert("origin", site.config.origin().parse().unwrap());
            r.headers_mut()
                .insert("cookie", format!("webts_dev={token}").parse().unwrap());
            r
        };
        let grant =
            json!({"server":"ts-local","identity":identity,"page":"fixture","name":"fixture"});
        let (mut denied, _) = tokio_tungstenite::connect_async(ws(&other)).await.unwrap();
        denied
            .send(Message::Text(grant.to_string().into()))
            .await
            .unwrap();
        let denied_reply = tokio::time::timeout(Duration::from_secs(5), denied.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(denied_reply.to_text().unwrap().contains("error"));
        denied.close(None).await.ok();
        assert_eq!(local.connections.total(), 0);
        let (mut browser, _) = tokio_tungstenite::connect_async(ws(&token)).await.unwrap();
        browser
            .send(Message::Text(grant.to_string().into()))
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
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        browser.close(None).await.ok();
        agent_stop.cancel();
        agent_task.await.unwrap();
        site.node_shutdown();
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(5), main_task)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), node_task)
            .await
            .unwrap()
            .unwrap();
        assert!(local.db.identities(owner).unwrap().is_empty());
    }
}
