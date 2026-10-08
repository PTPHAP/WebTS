use crate::{
    app::{Api, App},
    media::Media,
};
use anyhow::{Context, Result, bail};
use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::HeaderMap,
    response::Response,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tsclientlib::{
    Connection, DisconnectOptions, MessageTarget, OutCommandExt, StreamItem, events::Event,
};
use tsproto_packets::packets::{
    AudioData, CodecType, Direction, Flags, OutAudio, OutCommand, PacketType,
};
use tsproto_types::{Codec, CodecEncryptionMode};

struct Entry {
    owner: i64,
    session: String,
    identity: String,
    uid: String,
    target: String,
    server: String,
    page: String,
    port: u16,
    cancel: CancellationToken,
}
pub struct Connections {
    max: usize,
    shutting_down: AtomicBool,
    entries: Mutex<HashMap<String, Entry>>,
}
struct Lease {
    registry: Arc<Connections>,
    id: String,
    port: u16,
    cancel: CancellationToken,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.registry.entries.lock().unwrap().remove(&self.id);
    }
}
impl Connections {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            shutting_down: AtomicBool::new(false),
            entries: Mutex::new(HashMap::new()),
        }
    }
    pub fn cancel(&self, owner: i64, session: Option<&str>, identity: Option<&str>) {
        for e in self.entries.lock().unwrap().values() {
            if e.owner == owner
                && session.is_none_or(|s| s == e.session)
                && identity.is_none_or(|i| i == e.identity)
            {
                e.cancel.cancel();
            }
        }
    }
    fn reserve(self: &Arc<Self>, mut entry: Entry, min: u16, max: u16) -> Result<Lease> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(retryable("网关正在重启"));
        }
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.max {
            return Err(retryable("网关连接已满"));
        }
        if entries.values().any(|e| {
            (e.uid == entry.uid && e.target == entry.target)
                || (e.owner == entry.owner && e.page == entry.page)
        }) {
            return Err(retryable("该身份已连接此服务器，或当前页面仍有连接"));
        }
        let port = (min..=max)
            .find(|p| !entries.values().any(|e| e.port == *p))
            .ok_or_else(|| retryable("语音端口已用尽"))?;
        entry.port = port;
        let cancel = entry.cancel.clone();
        let id = crate::vault::token()?;
        entries.insert(id.clone(), entry);
        Ok(Lease {
            registry: self.clone(),
            id,
            port,
            cancel,
        })
    }
    pub fn cancel_disallowed(
        &self,
        old: &crate::settings::Settings,
        new: &crate::settings::Settings,
    ) {
        for entry in self.entries.lock().unwrap().values() {
            let permitted = if entry.server.is_empty() {
                new.allow_custom
            } else {
                old.servers
                    .iter()
                    .find(|s| s.id == entry.server)
                    .is_some_and(|previous| {
                        new.servers
                            .iter()
                            .any(|s| s.id == entry.server && s.address == previous.address)
                    })
            };
            if !permitted {
                entry.cancel.cancel();
            }
        }
    }
    pub fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        for e in self.entries.lock().unwrap().values() {
            e.cancel.cancel();
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Connect {
    server: String,
    #[serde(default)]
    address: String,
    identity: String,
    page: String,
    name: String,
    #[serde(default)]
    password: String,
}
pub async fn upgrade(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Api<Response> {
    app.session(&headers)?;
    let permit = app.sockets.clone().try_acquire_owned().map_err(|_| {
        crate::app::Error(axum::http::StatusCode::TOO_MANY_REQUESTS, "网关连接已满")
    })?;
    Ok(ws
        .max_message_size(256 * 1024)
        .max_frame_size(256 * 1024)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            run(app, headers, socket).await
        }))
}
fn emit(tx: &mpsc::Sender<Value>, value: Value) -> Result<()> {
    tx.try_send(value).context("网页接收过慢，连接已停止")
}
#[derive(Debug)]
struct Retryable(String);
impl std::fmt::Display for Retryable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Retryable {}
fn retryable(message: &str) -> anyhow::Error {
    anyhow::Error::new(Retryable(message.into()))
}
async fn run(app: Arc<App>, headers: HeaderMap, socket: WebSocket) {
    let (mut sink, mut input) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Value>(64);
    let done = CancellationToken::new();
    let writer_done = done.clone();
    let mut writer = tokio::spawn(async move {
        while let Some(value) = rx.recv().await {
            if !matches!(
                tokio::time::timeout(
                    Duration::from_secs(2),
                    sink.send(Message::Text(value.to_string().into())),
                )
                .await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
        writer_done.cancel();
        let _ = sink.close().await;
    });
    let result = bridge(&app, &headers, &mut input, &tx, &done).await;
    let retry = result
        .as_ref()
        .err()
        .is_some_and(|error| error.is::<Retryable>());
    if let Err(error) = result {
        let _ = emit(&tx, json!({"type":"error","message":error.to_string()}));
    }
    let _ = emit(&tx, json!({"type":"disconnected","retryable":retry}));
    drop(tx);
    if tokio::time::timeout(Duration::from_secs(3), &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
}
async fn bridge(
    app: &Arc<App>,
    headers: &HeaderMap,
    input: &mut futures::stream::SplitStream<WebSocket>,
    tx: &mpsc::Sender<Value>,
    done: &CancellationToken,
) -> Result<()> {
    let first = tokio::time::timeout(Duration::from_secs(10), input.next())
        .await?
        .context("网页连接已关闭")??;
    let Message::Text(first) = first else {
        bail!("连接请求格式无效");
    };
    let request: Connect =
        serde_json::from_str(&first).map_err(|_| anyhow::anyhow!("连接请求格式无效"))?;
    if !valid_nickname(&request.name) {
        bail!("显示昵称需要3–30个字符，且不能包含控制字符");
    }
    if request.password.len() > 256 || request.page.len() > 64 || request.page.is_empty() {
        bail!("连接参数无效");
    }
    let session = app
        .session(headers)
        .map_err(|_| anyhow::anyhow!("请重新登录"))?;
    let source = {
        let current = app.runtime.read().unwrap();
        if !request.address.is_empty() {
            if !request.server.is_empty() {
                bail!("自定义地址和服务器ID只能选一个");
            }
            if !current.settings.allow_custom {
                bail!("管理员已关闭自定义连接");
            }
            crate::settings::normalize_target(&request.address)
                .map_err(|_| anyhow::anyhow!("自定义地址无效，只允许公网主机和UDP端口"))?
        } else {
            current
                .settings
                .servers
                .iter()
                .find(|s| s.id == request.server)
                .context("服务器不在站长配置列表中")?
                .address
                .clone()
        }
    };
    let addresses = tsclientlib::resolver::resolve(source.clone());
    futures::pin_mut!(addresses);
    let target = tokio::time::timeout(Duration::from_secs(10), addresses.next())
        .await
        .map_err(|_| retryable("地址解析超时"))?
        .ok_or_else(|| retryable("地址无法解析"))?
        .map_err(|_| anyhow::anyhow!("地址无法解析或目标不被允许"))?;
    if !tsclientlib::resolver::is_public_addr(&target.ip()) {
        bail!("不能连接内网或回环地址");
    }
    let owner = session.user.id;
    let id = request.identity.clone();
    let (identity, uid) = app
        .work(move |a| {
            let record =
                a.db.identity(owner, &id)
                    .map_err(|_| crate::app::Error::bad("身份不存在"))?;
            let bytes = a.vault.open(owner, &id, &record.uid, &record.ciphertext)?;
            let value = crate::identity::parse(
                std::str::from_utf8(&bytes).map_err(|_| crate::app::Error::bad("身份无效"))?,
            )?;
            Ok((value, record.uid))
        })
        .await
        .map_err(|_| anyhow::anyhow!("身份无法读取"))?;
    let lease = {
        let current = app.runtime.read().unwrap();
        if request.server.is_empty() {
            if !current.settings.allow_custom {
                bail!("管理员已关闭自定义连接");
            }
        } else if !current
            .settings
            .servers
            .iter()
            .any(|s| s.id == request.server && s.address == source)
        {
            bail!("服务器设置已变化，请重新连接");
        }
        app.connections.reserve(
            Entry {
                owner,
                session: session.hash,
                identity: request.identity.clone(),
                uid,
                target: target.to_string(),
                server: request.server.clone(),
                page: request.page,
                port: 0,
                cancel: CancellationToken::new(),
            },
            app.config.rtc.udp_min,
            app.config.rtc.udp_max,
        )?
    };
    app.session(headers)
        .map_err(|_| anyhow::anyhow!("登录已失效"))?;
    app.db
        .identity(owner, &request.identity)
        .map_err(|_| anyhow::anyhow!("身份已删除"))?;
    emit(tx, json!({"type":"status","message":"正在连接 TeamSpeak…"}))?;
    let mut conn = Connection::build(target.to_string())
        .identity(identity)
        .name(request.name)
        .password(request.password)
        .input_hardware_enabled(true)
        .output_hardware_enabled(true)
        .log_commands(false)
        .log_packets(false)
        .log_udp_packets(false)
        .connect()
        .map_err(report_connection_error)?;
    let cancel = lease.cancel.clone();
    tokio::time::timeout(Duration::from_secs(30),async{loop{tokio::select!{_ = cancel.cancelled()=>{if app.connections.shutting_down.load(Ordering::Acquire){return Err(retryable("网关正在重启"));}bail!("连接已撤销")},_ = done.cancelled()=>bail!("网页连接已关闭"),event=async {conn.events().next().await}=>{match event.context("TeamSpeak连接已关闭")?.map_err(report_connection_error)?{StreamItem::BookEvents(_)=>break,StreamItem::IdentityLevelIncreasing(level)=>bail!("服务器要求身份安全等级至少为 {level}，请在原生客户端提高后重新导入"),_=>{}}}}}Ok::<_,anyhow::Error>(())}).await.map_err(|_|retryable("TeamSpeak握手超时，请检查UDP端口、防火墙和IP封禁"))??;
    enforce(&conn)?;
    command("channelsubscribeall", &[]).send_with_result(&mut conn)?;
    let (audio_tx, mut audio_rx) = mpsc::channel(8);
    let mut media =
        Media::new(app, owner, lease.port, tx.clone(), audio_tx, cancel.clone()).await?;
    let result = connected(
        app,
        headers,
        &mut conn,
        &mut media,
        input,
        tx,
        &cancel,
        done,
        &mut audio_rx,
    )
    .await;
    media.close().await;
    let _ = conn.disconnect(DisconnectOptions::new());
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while conn.events().next().await.is_some() {}
    })
    .await;
    result
}
fn valid_nickname(name: &str) -> bool {
    name.trim().chars().count() >= 3
        && name.chars().count() <= 30
        && !name.chars().any(char::is_control)
}

fn report_connection_error(error: tsclientlib::Error) -> anyhow::Error {
    let message = connection_error(&error);
    tracing::warn!(reason = %message, "TeamSpeak连接失败");
    if retry_connection_error(&error) {
        retryable(&message)
    } else {
        anyhow::anyhow!(message)
    }
}

fn retry_connection_error(error: &tsclientlib::Error) -> bool {
    use tsclientlib::Error;
    use tsproto_types::errors::Error as TsError;
    match error {
        Error::ConnectFailed { errors, .. } => errors.last().is_none_or(retry_connection_error),
        Error::ConnectTs(
            TsError::ServerMaxclientsReached | TsError::ClientIsFlooding | TsError::BanFlooding,
        ) => true,
        Error::Connect(protocol)
        | Error::ConnectionFailed(protocol)
        | Error::InitserverWait(protocol)
        | Error::SendClientinit(protocol) => matches!(
            protocol,
            tsproto::client::Error::TsProto(
                tsproto::Error::Timeout(_) | tsproto::Error::Network(_)
            )
        ),
        _ => false,
    }
}
fn connection_error(error: &tsclientlib::Error) -> String {
    use tsclientlib::Error;
    use tsproto_types::errors::Error as TsError;
    match error {
        Error::ConnectFailed { errors, .. } => errors
            .last()
            .map(connection_error)
            .unwrap_or_else(|| "TeamSpeak目标无法连接，请检查服务器地址和UDP端口".into()),
        Error::IdentityLevel(level) => {
            format!("服务器要求身份安全等级至少为 {level}，请在原生客户端提高后重新导入")
        }
        Error::IdentityLevelCorrupted { needed, have } => format!(
            "身份安全等级校验异常（服务器要求 {needed}，本地为 {have}），请从原生客户端重新导出并导入"
        ),
        Error::ConnectTs(reason) => {
            let message = match reason {
                TsError::ServerInvalidPassword | TsError::ClientInvalidPassword => {
                    "TeamSpeak服务器密码错误，请填写TS服务器密码（不是网站登录密码）"
                }
                TsError::ConnectFailedBanned => {
                    "TeamSpeak拒绝连接：网关IP或当前身份被封禁，请联系TS服务器管理员"
                }
                TsError::ClientIsFlooding | TsError::BanFlooding => {
                    "TeamSpeak拒绝连接：连接过于频繁或触发防刷限制，请稍后重试"
                }
                TsError::ServerMaxclientsReached => "TeamSpeak服务器连接人数已满",
                TsError::ClientTooManyClonesConnected => {
                    "TeamSpeak拒绝连接：同一身份连接数量已达上限，请先断开旧客户端"
                }
                TsError::ClientNicknameInuse => "TeamSpeak昵称已被使用，请更换昵称",
                TsError::ParameterInvalidSize => {
                    "TeamSpeak拒绝连接参数长度，请检查显示昵称是否为3–30个字符"
                }
                TsError::ClientCouldNotValidateIdentity => {
                    "TeamSpeak无法验证身份，请检查身份安全等级并从原生客户端重新导入"
                }
                TsError::ClientVersionOutdated | TsError::ServerVersionOutdated => {
                    "TeamSpeak协议版本不兼容，需要检查网关和TS服务器版本"
                }
                _ => return format!("TeamSpeak拒绝连接（{reason}，错误代码 {}）", *reason as u16),
            };
            message.into()
        }
        Error::Connect(protocol)
        | Error::ConnectionFailed(protocol)
        | Error::InitserverWait(protocol)
        | Error::SendClientinit(protocol) => match protocol {
            tsproto::client::Error::TsProto(tsproto::Error::Timeout(_)) => {
                "TeamSpeak UDP通信超时，请检查服务器地址、UDP端口、防火墙和网关IP封禁".into()
            }
            tsproto::client::Error::TsProto(tsproto::Error::Network(_)) => {
                "TeamSpeak UDP网络连接失败，请检查目标端口和网络访问规则".into()
            }
            tsproto::client::Error::OutdatedServer => {
                "TeamSpeak服务器协议过旧，当前网关无法兼容".into()
            }
            _ => "TeamSpeak握手失败：服务器协议或网络响应异常".into(),
        },
        Error::InitserverTimeout | Error::HandshakeTimeout => {
            "TeamSpeak握手超时，请检查UDP端口、防火墙和网关IP封禁".into()
        }
        Error::ResolveAddress(_) => "TeamSpeak地址无法解析或目标不被允许".into(),
        _ => "TeamSpeak连接失败：服务器协议或网络响应异常".into(),
    }
}

fn enforce(conn: &Connection) -> Result<()> {
    let state = conn.get_state()?;
    if state.server.codec_encryption_mode != CodecEncryptionMode::ForcedOn {
        bail!("服务器必须将语音加密设置为 Globally on；已停止连接");
    }
    if let Some(own) = state.clients.get(&state.own_client)
        && let Some(channel) = state.channels.get(&own.channel)
        && !matches!(channel.codec, Codec::OpusVoice | Codec::OpusMusic)
    {
        bail!("当前频道需使用Opus编码");
    }
    Ok(())
}
fn snapshot(conn: &Connection) -> Result<Value> {
    let state = conn.get_state()?;
    Ok(
        json!({"type":"state","server":state.server.name,"own":state.own_client.0,"canSpeak":conn.can_send_audio(),"channels":state.channels.values().map(|c|json!({"id":c.id.0,"parent":c.parent.0,"order":c.order.0,"name":c.name,"topic":c.topic,"password":c.has_password.unwrap_or(false),"description":c.optional_data.as_ref().map(|d|&d.description)})).collect::<Vec<_>>(),"members":state.clients.values().map(|c|json!({"id":c.id.0,"channel":c.channel.0,"name":c.name,"uid":c.uid.as_ref().map(|u|u.as_ref().to_string()),"avatarHash":if c.avatar_hash.len()==32&&c.avatar_hash.bytes().all(|b|b.is_ascii_hexdigit()){c.avatar_hash.as_str()}else{""},"muted":c.input_muted,"deafened":c.output_muted,"description":c.description,"talkPower":c.talk_power,"serverGroups":c.server_groups.iter().map(|g|g.0).collect::<Vec<_>>(),"channelGroup":c.channel_group.0})).collect::<Vec<_>>() }),
    )
}
// These borrows belong to a single connection actor; keeping ownership together
// avoids independent tasks that can outlive revocation.
#[allow(clippy::too_many_arguments)]
async fn connected(
    app: &Arc<App>,
    headers: &HeaderMap,
    conn: &mut Connection,
    media: &mut Media,
    input: &mut futures::stream::SplitStream<WebSocket>,
    tx: &mpsc::Sender<Value>,
    cancel: &CancellationToken,
    done: &CancellationToken,
    audio: &mut mpsc::Receiver<rtc::rtp::Packet>,
) -> Result<()> {
    let mut timer = tokio::time::interval(Duration::from_millis(100));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut changed = true;
    let mut transmit = false;
    let mut voice_open = false;
    let mut muted = false;
    let mut deafened = false;
    let mut whisper_clients = Vec::<u16>::new();
    let mut whisper_channels = Vec::<u64>::new();
    let mut sequence = 0u16;
    let mut pending = HashMap::<u16, (String, Instant)>::new();
    let mut rate = (Instant::now(), 0u32);
    let mut check = Instant::now() - Duration::from_secs(2);
    let mut network_check = Instant::now();
    let mut cipher = None;
    let mut speaking = HashMap::<u16, (Instant, Option<Instant>)>::new();
    let mut negotiated = Instant::now();
    let mut avatars = crate::avatar::Avatars::default();
    let mut described_channel = None;
    loop {
        tokio::select! {
            _=cancel.cancelled()=>{if app.connections.shutting_down.load(Ordering::Acquire){return Err(retryable("网关正在重启"));}break;},
            _=done.cancelled()=>break,
            _=timer.tick()=>{
                enforce(conn)?;
                let own_channel=conn.get_state()?.clients.get(&conn.get_state()?.own_client).map(|c|c.channel);
                if own_channel!=described_channel{if let Some(channel)=own_channel{command("channelgetdescription",&[("cid",channel.0.to_string())]).send(conn)?;}described_channel=own_channel;}
                for completed in avatars.expire(){avatar_completed(conn,&mut avatars,&mut pending,completed,tx)?;}
                if changed{let s=conn.get_state()?;speaking.retain(|id,_|*id==s.own_client.0||s.clients.keys().any(|client|client.0==*id));let ids:Vec<u16>=s.clients.keys().filter(|id|**id!=s.own_client).map(|id|id.0).collect();media.sync_speakers(&ids,tx).await?;emit(tx,snapshot(conn)?)?;changed=false;}
                if network_check.elapsed()>=Duration::from_secs(2){if let Ok(stats)=conn.get_network_stats(){emit(tx,json!({"type":"network","ts_rtt_ms":if stats.rtt.is_zero(){None}else{Some(stats.rtt.as_secs_f64()*1000.0)}}))?;}emit(tx,json!({"type":"heartbeat"}))?;network_check=Instant::now();}
                if media.dirty&&!media.negotiating{media.offer(tx).await?;negotiated=Instant::now();}
                if media.negotiating&&negotiated.elapsed()>Duration::from_secs(30){return Err(retryable("浏览器语音协商超时"));}
                if check.elapsed()>=Duration::from_secs(1){app.session(headers).map_err(|_|anyhow::anyhow!("登录已失效"))?;let actual=media.check_cipher().await?;if actual!=cipher{if let Some(value)=&actual{emit(tx,value.clone())?;}cipher=actual;}check=Instant::now();pending.retain(|_,(id,start)|{if start.elapsed()>Duration::from_secs(15){let _=emit(tx,json!({"type":"result","id":id,"ok":false,"message":"TeamSpeak操作响应超时"}));false}else{true}});}
            },
            packet=audio.recv()=>{let Some(packet)=packet else{return Err(retryable("语音通道已关闭"));};if transmit&&!muted&&media.secure.load(Ordering::Acquire)&&conn.can_send_audio(){if crate::media::opus_has_audio(&packet.payload){send_voice(conn,&mut sequence,&whisper_clients,&whisper_channels,&packet.payload)?;voice_open=true;let own=conn.get_state()?.own_client.0;if speech_activity(&mut speaking,own,Instant::now()){emit(tx,json!({"type":"speaking","client":own}))?;}}else{finish_voice(conn,&mut sequence,&whisper_clients,&whisper_channels,&mut voice_open,tx)?;}}},
            event=async {conn.events().next().await}=>{let event=event.context("TeamSpeak连接已结束，可能被踢出或服务器关闭")?.map_err(report_connection_error)?;if matches!(event,StreamItem::DisconnectedTemporarily(_)){return Err(retryable("TeamSpeak连接中断，正在自动重连"));}enforce(conn)?;match event{
                StreamItem::BookEvents(events)=>{for event in events{if let Event::Message{target,invoker,message}=event{let(scope,recipient)=match target{MessageTarget::Server=>("server",None),MessageTarget::Channel=>("channel",None),MessageTarget::Client(id)=>("client",Some(id.0)),MessageTarget::Poke(_)=>{emit(tx,json!({"type":"poke","from":invoker.id.0,"name":invoker.name,"text":message}))?;continue;}};emit(tx,json!({"type":"chat","scope":scope,"from":invoker.id.0,"name":invoker.name,"text":message,"target":recipient}))?;}else{changed=true;}}},
                StreamItem::AudioChange(_)=>changed=true,
                StreamItem::DisconnectedTemporarily(_)=>unreachable!(),
                StreamItem::FileDownload(handle,result)=>avatars.downloaded(handle.0,result,app.clone()),
                StreamItem::FileUpload(handle,result)=>avatars.uploaded(handle.0,result),
                StreamItem::FiletransferFailed(handle,_)=>{if let Some(completed)=avatars.failed(handle.0){avatar_completed(conn,&mut avatars,&mut pending,completed,tx)?;}},
                StreamItem::Audio(packet)=>{if packet.data().packet().header().flags().contains(Flags::UNENCRYPTED){bail!("检测到未加密语音，连接已停止");}match packet.data().data(){AudioData::S2C{id,from,codec,data}|AudioData::S2CWhisper{id,from,codec,data}if !deafened&&matches!(codec,CodecType::OpusVoice|CodecType::OpusMusic)=> {media.audio(*from,*id,data).await?;if data.is_empty(){if speaking.remove(from).is_some(){emit(tx,json!({"type":"speaking","client":from,"enabled":false}))?;}}else if crate::media::opus_has_audio(data)&&speech_activity(&mut speaking,*from,Instant::now()){emit(tx,json!({"type":"speaking","client":from}))?;}},_=>{}}},
                StreamItem::MessageResult(handle,result)=>if let Some((id,_))=pending.remove(&handle.0){if id.starts_with("avatar:")&&result.is_ok(){command("clientgetvariables",&[("clid",conn.get_state()?.own_client.0.to_string())]).send(conn)?;}emit(tx,command_result(&id,result))?;},
                _=>{}
            }},
            completed=avatars.tasks.join_next(),if !avatars.tasks.is_empty()=>{if let Some(Ok(completed))=completed{avatar_completed(conn,&mut avatars,&mut pending,completed,tx)?;}},
            message=input.next()=>{let Some(message)=message else{break;};let message=message?;let Message::Text(text)=message else{if matches!(message,Message::Close(_)){break;}continue;};
                if rate.0.elapsed()>Duration::from_secs(1){rate=(Instant::now(),0);}rate.1+=1;if rate.1>40{bail!("网页操作过于频繁");}
                let value:Value=serde_json::from_str(&text).map_err(|_|anyhow::anyhow!("网页请求格式错误"))?;
                match value["type"].as_str().unwrap_or(""){
                    "avatar_get"=>{let client=value["client"].as_u64().filter(|id|*id<=u16::MAX as u64).context("头像成员无效")? as u16;if let Err(error)=avatars.download(conn,client){emit(tx,json!({"type":"avatar","client":client,"uid":conn.get_state()?.clients.get(&tsproto_types::ClientId(client)).and_then(|c|c.uid.as_ref()).map(|u|u.to_string()),"hash":value["hash"].as_str().filter(|s|s.len()==32&&s.bytes().all(|b|b.is_ascii_hexdigit())).unwrap_or(""),"data":null,"error":error.to_string()}))?;}},
                    "avatar_upload"=>{let id=value["id"].as_str().filter(|id|id.starts_with("avatar:")&&id.len()<=64).context("头像操作ID无效")?.to_owned();let result=avatars.prepare_upload(app.clone(),id.clone(),value["data"].as_str().unwrap_or(""));if let Err(error)=result{emit(tx,json!({"type":"result","id":id,"ok":false,"message":error.to_string()}))?;}},
                    "channel_info"=>{let id=value["channel"].as_u64().context("频道无效")?;if conn.get_state()?.channels.contains_key(&tsproto_types::ChannelId(id)){command("channelgetdescription",&[("cid",id.to_string())]).send(conn)?;}},
                    "answer"=>media.answer(serde_json::from_value(value["description"].clone())?).await?,
                    "ice"=>media.ice(value["candidate"].clone()).await?,
                    "transmit"=>{transmit=value["enabled"].as_bool().unwrap_or(false);if !transmit{finish_voice(conn,&mut sequence,&whisper_clients,&whisper_channels,&mut voice_open,tx)?;}},
                    "whisper"=>{let clients=targets(&value["clients"],u16::MAX as u64)?;let channels=targets(&value["channels"],u64::MAX)?;let s=conn.get_state()?;if clients.iter().any(|id|!s.clients.keys().any(|c|u64::from(c.0)==*id))||channels.iter().any(|id|!s.channels.keys().any(|c|c.0==*id)){bail!("耳语目标不存在");}finish_voice(conn,&mut sequence,&whisper_clients,&whisper_channels,&mut voice_open,tx)?;whisper_clients=clients.into_iter().map(|c|c as u16).collect();whisper_channels=channels;emit(tx,json!({"type":"whisper","active":!whisper_clients.is_empty()||!whisper_channels.is_empty()}))?;},
                    "mute"=>{muted=value["muted"].as_bool().unwrap_or(false);deafened=value["deafened"].as_bool().unwrap_or(false);if muted{finish_voice(conn,&mut sequence,&whisper_clients,&whisper_channels,&mut voice_open,tx)?;}command("clientupdate",&[("client_input_muted",u8::from(muted).to_string()),("client_output_muted",u8::from(deafened).to_string())]).send_with_result(conn)?;},
                    "disconnect"=>break,
                    "command"=>{let id=value["id"].as_str().filter(|s|s.len()<=64).unwrap_or("").to_owned();if pending.len()>=32{bail!("待处理操作过多");}match user_command(conn,&value){Ok(cmd)=>{let handle=cmd.send_with_result(conn)?;pending.insert(handle.0,(id,Instant::now()));},Err(error)=>emit(tx,json!({"type":"result","id":id,"ok":false,"message":error.to_string()}))?,}},
                    _=>bail!("不支持的网页请求")
                }
            }
        }
    }
    Ok(())
}
fn avatar_completed(
    conn: &mut Connection,
    avatars: &mut crate::avatar::Avatars,
    pending: &mut HashMap<u16, (String, Instant)>,
    completed: crate::avatar::Completed,
    tx: &mpsc::Sender<Value>,
) -> Result<()> {
    use crate::avatar::Completed;
    match completed {
        Completed::Download(target, result) => {
            let state = conn.get_state()?;
            if state
                .clients
                .get(&tsproto_types::ClientId(target.client))
                .is_some_and(|c| {
                    c.uid.as_ref().is_some_and(|u| u.to_string() == target.uid)
                        && c.avatar_hash == target.hash
                })
            {
                let (data, error) = match result {
                    Ok(bytes) => (
                        Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes))),
                        None,
                    ),
                    Err(error) => (None, Some(error.to_string())),
                };
                emit(
                    tx,
                    json!({"type":"avatar","client":target.client,"uid":target.uid,"hash":target.hash,"data":data,"error":error}),
                )?;
            } else {
                emit(
                    tx,
                    json!({"type":"avatar","client":target.client,"uid":target.uid,"hash":target.hash,"data":null,"error":"头像版本已变化"}),
                )?;
            }
        }
        Completed::UploadReady(id, result) => {
            let result = result.and_then(|bytes| avatars.upload(conn, id.clone(), bytes));
            if let Err(error) = result {
                emit(
                    tx,
                    json!({"type":"result","id":id,"ok":false,"message":error.to_string()}),
                )?;
            }
        }
        Completed::Uploaded(id, result) => match result {
            Ok(hash) => {
                if pending.len() >= 32 {
                    emit(
                        tx,
                        json!({"type":"result","id":id,"ok":false,"message":"待处理操作过多，请重试"}),
                    )?;
                } else {
                    let handle = command("clientupdate", &[("client_flag_avatar", hash)])
                        .send_with_result(conn)?;
                    pending.insert(handle.0, (id, Instant::now()));
                }
            }
            Err(error) => emit(
                tx,
                json!({"type":"result","id":id,"ok":false,"message":error.to_string()}),
            )?,
        },
    }
    Ok(())
}
fn targets(value: &Value, max: u64) -> Result<Vec<u64>> {
    let list = value.as_array().context("耳语目标需为列表")?;
    if list.len() > 16 {
        bail!("耳语最多16个目标");
    }
    list.iter()
        .map(|v| {
            v.as_u64()
                .filter(|id| *id > 0 && *id <= max)
                .context("目标ID无效")
        })
        .collect()
}
fn send_voice(
    conn: &mut Connection,
    sequence: &mut u16,
    clients: &[u16],
    channels: &[u64],
    payload: &[u8],
) -> Result<()> {
    enforce(conn)?;
    let state = conn.get_state()?;
    let codec = if state
        .clients
        .get(&state.own_client)
        .and_then(|client| state.channels.get(&client.channel))
        .is_some_and(|channel| channel.codec == Codec::OpusMusic)
    {
        CodecType::OpusMusic
    } else {
        CodecType::OpusVoice
    };
    let data = if clients.is_empty() && channels.is_empty() {
        AudioData::C2S {
            id: *sequence,
            codec,
            data: payload,
        }
    } else {
        AudioData::C2SWhisper {
            id: *sequence,
            codec,
            clients: clients.to_vec(),
            channels: channels.to_vec(),
            data: payload,
        }
    };
    conn.send_audio(OutAudio::new(&data))?;
    *sequence = sequence.wrapping_add(1);
    Ok(())
}
fn finish_voice(
    conn: &mut Connection,
    sequence: &mut u16,
    clients: &[u16],
    channels: &[u64],
    open: &mut bool,
    tx: &mpsc::Sender<Value>,
) -> Result<()> {
    if *open {
        send_voice(conn, sequence, clients, channels, &[])?;
        *open = false;
        emit(
            tx,
            json!({"type":"speaking","client":conn.get_state()?.own_client.0,"enabled":false}),
        )?;
    }
    Ok(())
}
// Display activity from consecutive audio frames, not from an open microphone or
// isolated Opus DTX refresh frames. This never gates or delays transmitted speech.
fn speech_activity(
    states: &mut HashMap<u16, (Instant, Option<Instant>)>,
    client: u16,
    now: Instant,
) -> bool {
    let Some((previous, announced)) = states.get_mut(&client) else {
        states.insert(client, (now, None));
        return false;
    };
    let continuous = now.saturating_duration_since(*previous) <= Duration::from_millis(120);
    *previous = now;
    if continuous
        && announced
            .is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_millis(100))
    {
        *announced = Some(now);
        true
    } else {
        false
    }
}
fn command_result(id: &str, result: std::result::Result<(), tsclientlib::CommandError>) -> Value {
    let error = result.err();
    json!({"type":"result","id":id,"ok":error.is_none(),"code":error.as_ref().map(|e|e.error as u16),"message":error.map(|e|format!("TS拒绝操作：{}（错误代码{}）",e,e.error as u16))})
}

fn command(name: &str, args: &[(&str, String)]) -> OutCommand {
    let mut out = OutCommand::new(Direction::C2S, Flags::empty(), PacketType::Command, name);
    for (key, value) in args {
        out.write_arg(key, value);
    }
    out
}
fn user_command(conn: &Connection, value: &Value) -> Result<OutCommand> {
    let number = |key: &str, max: u64| -> Result<String> {
        Ok(value[key]
            .as_u64()
            .filter(|n| *n > 0 && *n <= max)
            .context("目标ID无效")?
            .to_string())
    };
    let text = |key: &str, max: usize| -> Result<String> {
        let s = value[key].as_str().context("缺少文本参数")?;
        if s.len() > max || s.contains('\0') {
            bail!("文本参数过长或无效");
        }
        Ok(s.to_owned())
    };
    let mut args = Vec::new();
    let name = match value["action"].as_str().unwrap_or("") {
        "move" => {
            args.push((
                "clid",
                value["client"]
                    .as_u64()
                    .unwrap_or(u64::from(conn.get_state()?.own_client.0))
                    .to_string(),
            ));
            args.push(("cid", number("channel", u64::MAX)?));
            args.push((
                "cpw",
                wire_password(
                    value["password"]
                        .as_str()
                        .filter(|s| s.len() <= 256)
                        .unwrap_or(""),
                ),
            ));
            "clientmove"
        }
        "chat" => {
            let mode = match value["scope"].as_str() {
                Some("private") => 1,
                Some("channel") => 2,
                Some("server") => 3,
                _ => bail!("聊天范围无效"),
            };
            args.push(("targetmode", mode.to_string()));
            args.push(("msg", text("text", 1024)?));
            if mode == 1 {
                args.push(("target", number("client", u16::MAX as u64)?));
            }
            "sendtextmessage"
        }
        "poke" => {
            args.push(("clid", number("client", u16::MAX as u64)?));
            args.push(("msg", text("text", 512)?));
            "clientpoke"
        }
        "kick" => {
            args.push(("clid", number("client", u16::MAX as u64)?));
            let reason = match value["scope"].as_str() {
                Some("channel") => 4,
                Some("server") => 5,
                _ => bail!("踢出范围无效"),
            };
            args.push(("reasonid", reason.to_string()));
            args.push(("reasonmsg", text("text", 512)?));
            "clientkick"
        }
        "channel_create" => {
            let title = text("name", 160)?;
            if title.trim().is_empty() {
                bail!("频道名称不能为空");
            }
            args.push(("channel_name", title));
            args.push(("channel_codec", "4".to_owned()));
            args.push(("cpid", value["parent"].as_u64().unwrap_or(0).to_string()));
            args.push(("channel_description", text("description", 4096)?));
            args.push(("channel_password", wire_password(&text("password", 256)?)));
            "channelcreate"
        }
        "channel_edit" => {
            args.push(("cid", number("channel", u64::MAX)?));
            for (key, wire, max) in [
                ("name", "channel_name", 160),
                ("description", "channel_description", 4096),
                ("password", "channel_password", 256),
            ] {
                if value.get(key).is_some() {
                    let value = text(key, max)?;
                    args.push((
                        wire,
                        if key == "password" {
                            wire_password(&value)
                        } else {
                            value
                        },
                    ));
                }
            }
            "channeledit"
        }
        "channel_delete" => {
            let channel = value["channel"].as_u64().context("频道ID无效")?;
            if conn
                .get_state()?
                .clients
                .values()
                .any(|c| c.channel.0 == channel)
            {
                bail!("只允许删除空频道");
            }
            args.push(("cid", number("channel", u64::MAX)?));
            args.push(("force", "0".to_owned()));
            "channeldelete"
        }
        _ => bail!("不支持的操作"),
    };
    Ok(command(name, &args))
}
fn wire_password(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        tsproto_types::crypto::encode_password(value.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::{command_result, connection_error, speech_activity, valid_nickname};
    use std::{
        collections::HashMap,
        time::{Duration, Instant},
    };
    use tsclientlib::Error;
    use tsproto_types::errors::Error as TsError;

    #[test]
    fn reconnect_retries_capacity_but_never_credentials_bans_or_permissions() {
        for reason in [TsError::ServerMaxclientsReached, TsError::ClientIsFlooding] {
            assert!(super::retry_connection_error(&Error::ConnectTs(reason)));
            assert!(
                super::report_connection_error(Error::ConnectTs(reason)).is::<super::Retryable>()
            );
        }
        for reason in [
            TsError::ConnectFailedBanned,
            TsError::ServerInvalidPassword,
            TsError::PermissionsClientInsufficient,
        ] {
            assert!(!super::retry_connection_error(&Error::ConnectTs(reason)));
            assert!(
                !super::report_connection_error(Error::ConnectTs(reason)).is::<super::Retryable>()
            );
        }
    }
    #[test]
    fn periodic_dtx_refresh_does_not_look_like_continuous_speech() {
        let mut states = HashMap::new();
        let start = Instant::now();
        for ms in [0, 400, 800, 1200] {
            assert!(!speech_activity(
                &mut states,
                1,
                start + Duration::from_millis(ms)
            ));
        }
        assert!(speech_activity(
            &mut states,
            1,
            start + Duration::from_millis(1220)
        ));
        assert!(!speech_activity(
            &mut states,
            1,
            start + Duration::from_millis(1240)
        ));
        assert!(speech_activity(
            &mut states,
            1,
            start + Duration::from_millis(1320)
        ));
        assert!(!speech_activity(
            &mut states,
            2,
            start + Duration::from_millis(1340)
        ));
        assert!(!speech_activity(
            &mut states,
            1,
            start + Duration::from_millis(1800)
        ));
    }
    #[test]
    fn channel_password_verdict_is_distinct_from_permission_and_server_password() {
        for error in [
            TsError::ChannelInvalidPassword,
            TsError::ServerInvalidPassword,
            TsError::PermissionsClientInsufficient,
        ] {
            let result = command_result(
                "move-test",
                Err(tsclientlib::CommandError {
                    error,
                    missing_permission: None,
                }),
            );
            assert_eq!(result["id"], "move-test");
            assert_eq!(result["ok"], false);
            assert_eq!(result["code"], error as u16);
            assert_eq!(
                result["code"] == 781,
                error == TsError::ChannelInvalidPassword
            );
        }
        let result = command_result("success", Ok(()));
        assert_eq!(result["ok"], true);
        assert!(result["code"].is_null());
    }
    #[test]
    fn nickname_matches_ts_unicode_length_limits() {
        assert!(!valid_nickname("测试"));
        assert!(valid_nickname("测试用户"));
        assert!(valid_nickname(&"测".repeat(30)));
        assert!(!valid_nickname(&"测".repeat(31)));
        assert!(!valid_nickname("   "));
        assert!(!valid_nickname("测试\n用户"));
    }

    #[test]
    fn handshake_failure_preserves_safe_actionable_reason() {
        let timeout = Error::ConnectFailed {
            address: "private-target.invalid:9987".into(),
            errors: vec![Error::Connect(tsproto::client::Error::TsProto(
                tsproto::Error::Timeout("Packet was not acked"),
            ))],
        };
        let message = connection_error(&timeout);
        assert!(
            message.contains("UDP") && message.contains("超时"),
            "{message}"
        );
        assert!(!message.contains("密码") && !message.contains("private-target"));
        let message = connection_error(&Error::IdentityLevel(30));
        assert!(message.contains("30") && message.contains("重新导入"));
        let message = connection_error(&Error::ConnectTs(TsError::ServerInvalidPassword));
        assert!(message.contains("服务器密码") && !message.contains("安全等级"));
        let message = connection_error(&Error::ConnectTs(TsError::ConnectFailedBanned));
        assert!(message.contains("封禁"));
    }
}
