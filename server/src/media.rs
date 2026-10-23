use crate::app::{Api, App};
use anyhow::{Context, Result, bail};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::Bytes;
use hmac::{Hmac, Mac};
use rtc::{
    media_stream::MediaStreamTrack,
    rtp::{Header, Packet},
    rtp_transceiver::{
        RTCRtpTransceiverDirection, RTCRtpTransceiverInit,
        rtp_sender::{
            RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
            RtpCodecKind,
        },
    },
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

// Recognition and WSS control can arrive after RTP. Keep only a short, bounded
// Opus lead-in while automatic speech is idle; PTT/mute/permission changes clear it.
#[derive(Default)]
pub(crate) struct SpeechLead(VecDeque<(Instant, Bytes)>);
impl SpeechLead {
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
    pub(crate) fn push(&mut self, payload: Bytes, now: Instant) {
        self.expire(now);
        self.0.push_back((now, payload));
        while self.0.len() > 32
            || self.0.iter().map(|(_, p)| p.len()).sum::<usize>() > 16384
            || self
                .0
                .iter()
                .map(|(_, p)| opus_samples(p).unwrap_or(9601))
                .sum::<u32>()
                > 9600
        {
            self.0.pop_front();
        }
    }
    fn expire(&mut self, now: Instant) {
        while self
            .0
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > Duration::from_millis(200))
        {
            self.0.pop_front();
        }
    }
    pub(crate) fn take(&mut self, now: Instant) -> Vec<Bytes> {
        self.expire(now);
        self.0.drain(..).map(|(_, p)| p).collect()
    }
}
use tokio::sync::mpsc;
#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransmitControl {
    #[serde(rename = "type")]
    kind: String,
    pub enabled: bool,
    pub pre_roll: bool,
    pub sequence: u64,
}
impl TransmitControl {
    fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > 128 {
            return None;
        }
        let value: Self = serde_json::from_slice(bytes).ok()?;
        (value.kind == "transmit" && value.sequence > 0 && value.sequence <= 9_007_199_254_740_991)
            .then_some(value)
    }
}
#[derive(Clone)]
pub(crate) enum AudioControl {
    Transmit(TransmitControl),
    Closed(u64),
}
pub(crate) fn control_sequence(last: &mut u64, sequence: Option<u64>) -> bool {
    match sequence {
        Some(next) if next > *last && next <= 9_007_199_254_740_991 => {
            *last = next;
            true
        }
        None => *last == 0,
        _ => false,
    }
}
use tokio_util::sync::CancellationToken;
use webrtc::{
    media_stream::{
        track_local::{TrackLocal, static_rtp::TrackLocalStaticRTP},
        track_remote::{TrackRemote, TrackRemoteEvent},
    },
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCBundlePolicy,
        RTCConfigurationBuilder, RTCIceServer, RTCPeerConnectionIceEvent, RTCPeerConnectionState,
        RTCSessionDescription, RTCStatsReportEntry, SettingEngineBuilder, SrtpProtectionProfile,
        StatsSelector,
    },
    rtp_transceiver::RtpSender,
    runtime::TokioRuntime,
};

// Internet deployment must not probe LAN/metadata endpoints supplied by users.
fn public_candidate(candidate: &str) -> bool {
    let fields: Vec<_> = candidate.split_ascii_whitespace().collect();
    fields.len() >= 8
        && fields[2].eq_ignore_ascii_case("udp")
        && fields[5].parse::<u16>().is_ok_and(|port| port != 0)
        && fields[4]
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| tsclientlib::resolver::is_public_addr(&ip))
}
fn public_sdp(sdp: &str) -> String {
    sdp.lines()
        .filter(|line| {
            line.trim()
                .strip_prefix("a=")
                .map(str::trim)
                .is_none_or(|attribute| {
                    !attribute.starts_with("candidate:") || public_candidate(attribute)
                })
        })
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n"
}

// webrtc 0.21 gathers interface addresses without applying nat_1to1_ips.
// Map only the advertised address; ICE must keep its actual bound socket address.
fn advertised_candidate(candidate: &str, public_ip: Option<IpAddr>) -> String {
    let Some(ip) = public_ip else {
        return candidate.to_owned();
    };
    let mut fields: Vec<_> = candidate.split_ascii_whitespace().collect();
    if fields.len() >= 8
        && fields[0].trim_start_matches("a=").starts_with("candidate:")
        && fields[2].eq_ignore_ascii_case("udp")
        && fields[6] == "typ"
        && fields[7] == "host"
        && fields[4]
            .parse::<IpAddr>()
            .is_ok_and(|local| local.is_ipv4() == ip.is_ipv4())
    {
        let address = ip.to_string();
        fields[4] = &address;
        return fields.join(" ");
    }
    candidate.to_owned()
}

fn advertised_sdp(sdp: &str, public_ip: Option<IpAddr>) -> String {
    if public_ip.is_none() {
        return sdp.to_owned();
    }
    sdp.lines()
        .map(|line| advertised_candidate(line, public_ip))
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n"
}

pub fn ice_servers(app: &App, owner: i64) -> Result<Vec<RTCIceServer>> {
    if let (Some(url), Some(path)) = (&app.config.rtc.turn_url, &app.config.rtc.turn_secret_file) {
        let secret = zeroize::Zeroizing::new(std::fs::read_to_string(path)?);
        let username = format!("{}:webts-{owner}", crate::db::now() + 3600);
        let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret.trim_end().as_bytes())?;
        mac.update(username.as_bytes());
        Ok(vec![RTCIceServer {
            urls: vec![url.clone()],
            username,
            credential: STANDARD.encode(mac.finalize().into_bytes()),
        }])
    } else {
        Ok(vec![])
    }
}
#[derive(serde::Deserialize)]
pub struct VoiceRoute {
    pub server: Option<String>,
}
pub async fn ice_config(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(route): Query<VoiceRoute>,
) -> Api<Json<Value>> {
    let owner = app.session_async(&headers).await?.user.id;
    if let Some(id) = route.server {
        let source = app
            .runtime
            .read()
            .unwrap()
            .settings
            .servers
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.address.clone());
        let source = source.ok_or_else(|| crate::app::Error::bad("服务器不在配置列表中"))?;
        if let Some(node) = crate::node::binding(&app, &id, &source)? {
            return Ok(Json(node.ice(&app, owner).await.map_err(|_| {
                crate::app::Error(
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "语音节点暂不可用",
                )
            })?));
        }
    }
    Ok(Json(
        json!({"iceServers":ice_servers(&app,owner)?.iter().map(|s|json!({"urls":s.urls,"username":s.username,"credential":s.credential})).collect::<Vec<_>>()}),
    ))
}
struct Handler {
    signals: mpsc::Sender<Value>,
    audio: mpsc::Sender<InboundAudio>,
    secure: Arc<AtomicBool>,
    cancel: CancellationToken,
    unexpected_channels: AtomicUsize,
    public_ip: Option<IpAddr>,
}
#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_data_channel(&self, channel: Arc<dyn webrtc::data_channel::DataChannel>) {
        if self.unexpected_channels.fetch_add(1, Ordering::Relaxed) >= 4 {
            self.secure.store(false, Ordering::Release);
            self.cancel.cancel();
        }
        let _ = channel.close().await;
    }
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        if let Ok(mut candidate) = event.candidate.to_json() {
            candidate.candidate = advertised_candidate(&candidate.candidate, self.public_ip);
            let _ = self
                .signals
                .try_send(json!({"type":"ice","candidate":candidate}));
        }
    }
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        let _ = self
            .signals
            .try_send(json!({"type":"rtc_state","state":state.to_string()}));
        if matches!(
            state,
            RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Closed
                | RTCPeerConnectionState::Disconnected
        ) {
            self.secure.store(false, Ordering::Release);
        }
        if matches!(
            state,
            RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed
        ) {
            self.cancel.cancel();
        }
    }
    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let tx = self.audio.clone();
        let secure = self.secure.clone();
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            let Some(ssrc) = track.ssrcs().await.first().copied() else {
                return;
            };
            let Some(codec) = track.codec(ssrc).await else {
                return;
            };
            if !codec.mime_type.eq_ignore_ascii_case("audio/opus") || codec.clock_rate != 48000 {
                return;
            }
            loop {
                tokio::select! {_ = cancel.cancelled()=>break,event=track.poll()=>{
                    let Some(event)=event else{break;};if let TrackRemoteEvent::OnRtpPacket(packet)=event
                        && secure.load(Ordering::Acquire)&&packet.payload.len()<=1276&&opus_samples(&packet.payload).is_ok(){let _=tx.try_send(InboundAudio{packet,received_at:Instant::now()});}
                }}
            }
        });
    }
}
pub struct InboundAudio {
    pub packet: Packet,
    pub received_at: Instant,
}
#[derive(Default)]
pub struct AudioQueueStats {
    count: u64,
    micros: u128,
    buckets: [u64; 21],
    dropped: u64,
}
impl AudioQueueStats {
    // The queue already has eight slots. Never forward old speech after a stall.
    pub fn observe(&mut self, received_at: Instant, now: Instant) -> bool {
        let age = now.saturating_duration_since(received_at);
        if age > Duration::from_millis(200) {
            self.dropped += 1;
            return false;
        }
        self.count += 1;
        self.micros += age.as_micros();
        self.buckets[age.as_nanos().div_ceil(10_000_000).min(20) as usize] += 1;
        true
    }
    pub fn take(&mut self, seconds: f64) -> Value {
        let rank = self.count.saturating_mul(95).div_ceil(100);
        let mut cumulative = 0;
        let p95 = if self.count > 0 {
            self.buckets.iter().enumerate().find_map(|(i, n)| {
                cumulative += n;
                (cumulative >= rank).then_some(i * 10)
            })
        } else {
            None
        };
        let value = json!({"type":"audio_queue","window_seconds":seconds,"received":self.count+self.dropped,"dropped":self.dropped,"mean_ms":if self.count>0{Some(self.micros as f64/self.count as f64/1000.0)}else{None},"p95_upper_ms":p95});
        *self = Self::default();
        value
    }
}
struct Speaker {
    track: Arc<TrackLocalStaticRTP>,
    sender: Arc<dyn RtpSender>,
    ssrc: u32,
    seq: Option<u16>,
    sequence_offset: u16,
    timestamp: u32,
    samples: u32,
    received: u64,
    stable_frames: u16,
    last: Instant,
    clock: Instant,
    ended: bool,
    ready: bool,
    pending: VecDeque<(Instant, u16, Bytes)>,
}
pub struct Media {
    pub(crate) control: tokio::sync::watch::Receiver<Option<AudioControl>>,
    control_task: tokio::task::JoinHandle<()>,
    pub peer: Arc<dyn PeerConnection>,
    pub secure: Arc<AtomicBool>,
    cancel: CancellationToken,
    speakers: HashMap<u16, Speaker>,
    pub negotiating: bool,
    pub dirty: bool,
    local_ice: bool,
    public_ip: Option<IpAddr>,
    signals: mpsc::Sender<Value>,
    offered_speakers: Vec<u16>,
    invalid_audio_notice: Instant,
}
impl Media {
    pub async fn new(
        app: &App,
        owner: i64,
        port: u16,
        signals: mpsc::Sender<Value>,
        audio: mpsc::Sender<InboundAudio>,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let mut engine = webrtc::peer_connection::MediaEngine::default();
        engine.register_codec(
            RTCRtpCodecParameters {
                rtp_codec: opus_codec(),
                payload_type: 111,
            },
            RtpCodecKind::Audio,
        )?;
        let registry = webrtc::peer_connection::register_default_interceptors(
            webrtc::peer_connection::Registry::new(),
            &mut engine,
        )?;
        let public_ip = (!app.config.rtc.public_ip.is_empty())
            .then(|| app.config.rtc.public_ip.parse::<IpAddr>())
            .transpose()
            .context("WebRTC公布地址必须为有效IP")?;
        let settings = SettingEngineBuilder::new()
            .with_sctp_max_message_size(
                rtc::peer_connection::configuration::setting_engine::SctpMaxMessageSize::Bounded(
                    1024,
                ),
            )
            .with_sctp_max_receive_buffer_size(8192)
            .with_srtp_protection_profiles(vec![
                SrtpProtectionProfile::Srtp_Aead_Aes_256_Gcm,
                SrtpProtectionProfile::Srtp_Aead_Aes_128_Gcm,
            ]);
        let secure = Arc::new(AtomicBool::new(false));
        let peer = PeerConnectionBuilder::new()
            .with_data_channel_send_buffer_limit(4096)
            .with_media_engine(engine)
            .with_interceptor_registry(registry)
            .with_configuration(
                RTCConfigurationBuilder::new()
                    .with_bundle_policy(RTCBundlePolicy::MaxBundle)
                    .with_ice_servers(ice_servers(app, owner)?)
                    .build(),
            )
            .with_setting_engine(settings.build())
            .with_runtime(Arc::new(TokioRuntime))
            .with_handler(Arc::new(Handler {
                signals: signals.clone(),
                audio,
                secure: secure.clone(),
                cancel: cancel.clone(),
                unexpected_channels: AtomicUsize::new(0),
                public_ip,
            }))
            .with_udp_addrs(vec![format!("0.0.0.0:{port}")])
            .build()
            .await?;
        peer.add_transceiver_from_kind(
            RtpCodecKind::Audio,
            Some(RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Recvonly,
                ..Default::default()
            }),
        )
        .await?;
        let channel = peer
            .create_data_channel("webts-audio-control-v1", None)
            .await?;
        let (control_tx, control) = tokio::sync::watch::channel(None);
        let stop = cancel.clone();
        let control_task = tokio::spawn(async move {
            use webrtc::data_channel::DataChannelEvent;
            let mut rate = (Instant::now(), 0u32);
            let mut sequence = 0;
            loop {
                let event = tokio::select! {_=stop.cancelled()=>break,event=channel.poll()=>event};
                match event {
                    Some(DataChannelEvent::OnOpen) => {
                        if !matches!(
                            tokio::time::timeout(
                                Duration::from_secs(1),
                                channel.send_text("{\"type\":\"audio_control_ready\"}")
                            )
                            .await,
                            Ok(Ok(_))
                        ) {
                            break;
                        }
                    }
                    Some(DataChannelEvent::OnMessage(message)) => {
                        if rate.0.elapsed() >= Duration::from_secs(1) {
                            rate = (Instant::now(), 0);
                        }
                        rate.1 += 1;
                        let Some(value) = TransmitControl::parse(&message.data) else {
                            break;
                        };
                        if rate.1 > 40 {
                            break;
                        }
                        sequence = sequence.max(value.sequence);
                        control_tx.send_replace(Some(AudioControl::Transmit(value)));
                    }
                    Some(
                        DataChannelEvent::OnError
                        | DataChannelEvent::OnClosing
                        | DataChannelEvent::OnClose,
                    )
                    | None => break,
                    _ => {}
                }
            }
            control_tx.send_replace(Some(AudioControl::Closed(sequence)));
            let _ = tokio::time::timeout(Duration::from_secs(1), channel.close()).await;
        });
        Ok(Self {
            control,
            control_task,
            peer: Arc::new(peer),
            secure,
            cancel,
            speakers: HashMap::new(),
            negotiating: false,
            dirty: true,
            local_ice: app.config.allow_insecure_localhost
                && app.config.public_url.starts_with("http://localhost:"),
            public_ip,
            signals,
            offered_speakers: Vec::new(),
            invalid_audio_notice: Instant::now() - Duration::from_secs(5),
        })
    }
    pub async fn sync_speakers(
        &mut self,
        clients: &[u16],
        signals: &mpsc::Sender<Value>,
    ) -> Result<()> {
        let removed: Vec<_> = self
            .speakers
            .keys()
            .copied()
            .filter(|id| !clients.contains(id))
            .collect();
        for id in removed {
            if let Some(s) = self.speakers.remove(&id) {
                self.peer.remove_track(&s.sender).await?;
                self.dirty = true;
            }
        }
        if clients.len() > 128 {
            bail!("频道成员超过首版语音轨道上限128");
        }
        for &id in clients {
            if self.speakers.contains_key(&id) {
                continue;
            }
            let mut random = [0; 8];
            getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("随机源不可用"))?;
            let ssrc = u32::from_le_bytes(random[..4].try_into()?);
            let track_id = format!("ts-{id}-{}", hex::encode(&random[4..]));
            let track = Arc::new(TrackLocalStaticRTP::new(MediaStreamTrack::new(
                "webts".to_owned(),
                track_id.clone(),
                format!("TS member {id}"),
                RtpCodecKind::Audio,
                vec![RTCRtpEncodingParameters {
                    rtp_coding_parameters: RTCRtpCodingParameters {
                        ssrc: Some(ssrc),
                        ..Default::default()
                    },
                    codec: opus_codec(),
                    ..Default::default()
                }],
            )));
            let sender = self
                .peer
                .add_track(track.clone() as Arc<dyn TrackLocal>)
                .await?;
            signals
                .try_send(json!({"type":"track","client":id,"track":track_id}))
                .map_err(|_| anyhow::anyhow!("信令队列已满"))?;
            self.speakers.insert(
                id,
                Speaker {
                    track,
                    sender,
                    ssrc,
                    seq: None,
                    sequence_offset: 0,
                    timestamp: 0,
                    samples: 960,
                    received: 0,
                    stable_frames: 0,
                    last: Instant::now(),
                    clock: Instant::now(),
                    ended: true,
                    ready: false,
                    pending: VecDeque::new(),
                },
            );
            self.dirty = true;
        }
        Ok(())
    }
    pub async fn offer(&mut self, signals: &mpsc::Sender<Value>) -> Result<()> {
        if self.negotiating || !self.dirty {
            return Ok(());
        }
        let mut offer = self.peer.create_offer(None).await?;
        self.peer.set_local_description(offer.clone()).await?;
        offer.sdp = advertised_sdp(&offer.sdp, self.public_ip);
        signals
            .try_send(json!({"type":"offer","description":offer}))
            .map_err(|_| anyhow::anyhow!("信令队列已满"))?;
        self.negotiating = true;
        self.offered_speakers = self.speakers.keys().copied().collect();
        self.dirty = false;
        Ok(())
    }
    pub async fn answer(&mut self, mut answer: RTCSessionDescription) -> Result<()> {
        if !self.local_ice {
            answer.sdp = public_sdp(&answer.sdp);
        }
        self.peer.set_remote_description(answer).await?;
        self.negotiating = false;
        for id in std::mem::take(&mut self.offered_speakers) {
            let Some(speaker) = self.speakers.get_mut(&id) else {
                continue;
            };
            speaker.ready = true;
            let pending = std::mem::take(&mut speaker.pending);
            for (at, sequence, data) in pending {
                if at.elapsed() <= Duration::from_millis(200) {
                    self.audio(id, sequence, &data).await?;
                }
            }
        }
        Ok(())
    }
    pub async fn ice(&self, value: Value) -> Result<()> {
        let candidate: rtc::peer_connection::transport::RTCIceCandidateInit =
            serde_json::from_value(value)?;
        if self.local_ice
            || candidate.candidate.is_empty()
            || public_candidate(&candidate.candidate)
        {
            self.peer.add_ice_candidate(candidate).await?;
        }
        Ok(())
    }
    pub async fn check_cipher(&self) -> Result<Option<Value>> {
        let stats = self
            .peer
            .get_stats(Instant::now(), StatsSelector::None)
            .await;
        for entry in stats.iter() {
            if let RTCStatsReportEntry::Transport(t) = entry {
                if t.srtp_cipher.is_empty() {
                    continue;
                }
                if !matches!(
                    t.srtp_cipher.as_str(),
                    "SRTP_AEAD_AES_128_GCM" | "SRTP_AEAD_AES_256_GCM"
                ) {
                    self.secure.store(false, Ordering::Release);
                    bail!("WebRTC协商结果低于AES-GCM加密下限");
                }
                self.secure.store(true, Ordering::Release);
                return Ok(Some(
                    json!({"type":"encryption","browser":t.srtp_cipher,"dtls":t.dtls_cipher,"teamspeak":"AES-128-EAX","forced":true}),
                ));
            }
        }
        Ok(None)
    }
    pub fn recent_speakers(&self) -> impl Iterator<Item = u16> + '_ {
        self.speakers
            .iter()
            .filter(|(_, s)| s.seq.is_some() && s.last.elapsed() < Duration::from_secs(30))
            .map(|(&id, _)| id)
    }
    pub async fn audio(&mut self, from: u16, sequence: u16, data: &[u8]) -> Result<bool> {
        if !self.secure.load(Ordering::Acquire) {
            return Ok(false);
        }
        // TS uses both empty and single-byte packets to end a speech stream.
        // This boundary is TS-specific; browser RTP Opus keeps its own validation.
        let samples = if data.len() <= 1 {
            None
        } else {
            let Ok(samples) = opus_samples(data) else {
                if self.invalid_audio_notice.elapsed() >= Duration::from_secs(5) {
                    tracing::warn!("已丢弃异常Opus语音帧；连接保持，未记录音频内容");
                    self.invalid_audio_notice = Instant::now();
                }
                return Ok(false);
            };
            Some(samples)
        };
        if !self.speakers.contains_key(&from) {
            if samples.is_none() || self.speakers.len() >= 128 {
                return Ok(false);
            }
            // A validated off-channel whisper creates a track only when audible.
            let mut clients: Vec<_> = self.speakers.keys().copied().collect();
            clients.push(from);
            self.sync_speakers(&clients, &self.signals.clone()).await?;
        }
        let Some(s) = self.speakers.get_mut(&from) else {
            return Ok(false);
        };
        if !s.ready {
            while s
                .pending
                .front()
                .is_some_and(|(at, _, _)| at.elapsed() > Duration::from_millis(200))
            {
                s.pending.pop_front();
            }
            if s.pending.len() < 8 && data.len() <= 2048 {
                s.pending
                    .push_back((Instant::now(), sequence, Bytes::copy_from_slice(data)));
            }
            return Ok(true);
        }
        let elapsed = s.last.elapsed();
        let marker = s.ended || elapsed > Duration::from_millis(200);
        let delta = s
            .seq
            .map(|previous| sequence.wrapping_sub(previous))
            .unwrap_or(1);
        if delta == 0 {
            return Ok(false);
        }
        let timestamp = if delta >= 0x8000 {
            let behind = 0u16.wrapping_sub(delta);
            // RTP can recover reordered frames without waiting in another queue.
            // Never cross a TS end marker or infer a different frame-duration clock.
            if marker
                || samples != Some(s.samples)
                || behind > s.stable_frames
                || behind >= 64
                || s.received & (1u64 << behind) != 0
            {
                return Ok(false);
            }
            s.received |= 1u64 << behind;
            s.timestamp.wrapping_sub(u32::from(behind) * s.samples)
        } else {
            let Some(samples) = samples else {
                // A TS end marker has a sequence number but no audio clock.
                // Keep the last real frame's time: markers may arrive immediately
                // or long after it, and must not add or erase elapsed silence.
                s.seq = Some(sequence);
                s.received = 1;
                s.stable_frames = 0;
                s.sequence_offset = s.sequence_offset.wrapping_add(delta);
                s.ended = true;
                return Ok(true);
            };
            if s.seq.is_some() {
                if marker {
                    // TS may advance its voice ID while not transmitting PCM.
                    // A new speech epoch must not turn that pause into RTP loss.
                    s.sequence_offset = s.sequence_offset.wrapping_add(delta - 1);
                }
                // Preserve the explicit speech boundary and the idle recovery
                // fallback: TS end markers can be lost on the UDP link.
                let advance = if marker {
                    (s.clock.elapsed().as_nanos() * 48 / 1_000_000).max(u128::from(s.samples))
                } else {
                    u128::from(delta) * u128::from(s.samples)
                };
                s.timestamp = s.timestamp.wrapping_add(advance as u32);
                // A burst advances the sample clock ahead of its arrival time.
                // Preserve that anchor so the next pause cannot count it again.
                s.clock += Duration::from_nanos((advance * 1_000_000_000 / 48_000) as u64);
            } else {
                s.clock = Instant::now();
            }
            s.received = if marker || delta >= 64 {
                1
            } else {
                (s.received << delta) | 1
            };
            s.stable_frames = if marker || samples != s.samples {
                0
            } else {
                (s.stable_frames + delta).min(63)
            };
            s.seq = Some(sequence);
            s.last = Instant::now();
            s.samples = samples;
            s.ended = false;
            s.timestamp
        };
        let packet = Packet {
            header: Header {
                version: 2,
                payload_type: 111,
                sequence_number: sequence.wrapping_sub(s.sequence_offset),
                timestamp,
                marker,
                ssrc: s.ssrc,
                ..Default::default()
            },
            payload: Bytes::copy_from_slice(data),
        };
        let _ = tokio::time::timeout(Duration::from_millis(20), s.track.write_rtp(packet)).await;
        Ok(true)
    }
    pub async fn close(&self) {
        self.control_task.abort();
        self.secure.store(false, Ordering::Release);
        self.cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), self.peer.close()).await;
    }
}
impl Drop for Media {
    fn drop(&mut self) {
        self.secure.store(false, Ordering::Release);
        self.cancel.cancel();
        self.control_task.abort();
    }
}
fn opus_codec() -> RTCRtpCodec {
    RTCRtpCodec {
        mime_type: "audio/opus".to_owned(),
        clock_rate: 48000,
        channels: 2,
        sdp_fmtp_line: "minptime=10;useinbandfec=1;usedtx=1".to_owned(),
        rtcp_feedback: vec![],
    }
}
// DTX header-only packets contain no encoded audio. Do not treat them as TS speech.
// Preserve short valid frames: payload length alone cannot identify silence.
pub fn opus_has_audio(data: &[u8]) -> bool {
    match data {
        [] => false,
        [toc] => !matches!(toc & 3, 0 | 1),
        [toc, count] => {
            !((toc & 3 == 2 && *count == 0)
                || (toc & 3 == 3 && count & 0xc0 == 0 && count & 63 != 0))
        }
        _ => true,
    }
}
pub fn opus_samples(data: &[u8]) -> Result<u32> {
    let Some(&toc) = data.first() else {
        bail!("空Opus包");
    };
    let config = toc >> 3;
    let samples = if config >= 16 {
        120 << (config & 3)
    } else if config >= 12 {
        480 << (config & 1)
    } else {
        [480, 960, 1920, 2880][usize::from(config & 3)]
    };
    let count = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => u32::from(*data.get(1).ok_or_else(|| anyhow::anyhow!("Opus帧数缺失"))? & 63),
    };
    if count == 0 || samples * count > 5760 {
        bail!("Opus帧长度无效");
    }
    Ok(samples * count)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn configured_public_ip_is_advertised_by_real_media_gathering() {
        let (_dir, source, _) = crate::app::tests::fixture();
        let mut config = source.config.clone();
        config.rtc.public_ip = "8.8.4.4".into();
        let app = App::node_runtime(config).unwrap();
        let (signals, mut events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(&app, 1, 0, signals.clone(), audio, CancellationToken::new())
            .await
            .unwrap();
        media.offer(&signals).await.unwrap();
        let candidate = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let event = events.recv().await.unwrap();
                if event["type"] == "ice" {
                    break event["candidate"]["candidate"].as_str().unwrap().to_owned();
                }
            }
        })
        .await
        .unwrap();
        media.close().await;
        assert_eq!(
            candidate.split_ascii_whitespace().nth(4),
            Some("8.8.4.4"),
            "configured public IP must replace the private interface in browser signalling: {candidate}"
        );
    }
    #[test]
    fn public_mapping_preserves_ports_extensions_other_candidates_and_sdp_security() {
        let ip = Some("8.8.4.4".parse().unwrap());
        let host = "candidate:abc 1 udp 2130706431 10.0.0.2 50003 typ host generation 0 ufrag test";
        let mapped = host.replace("10.0.0.2", "8.8.4.4");
        assert_eq!(advertised_candidate(host, ip), mapped);
        assert_eq!(advertised_candidate(&mapped, ip), mapped);
        assert_eq!(advertised_candidate(host, None), host);
        for value in [
            "candidate:a 1 udp 1 1.1.1.1 3478 typ srflx raddr 10.0.0.2 rport 50003",
            "candidate:a 1 udp 1 1.1.1.1 3478 typ relay raddr 10.0.0.2 rport 50003",
            "candidate:a 1 tcp 1 10.0.0.2 50003 typ host tcptype passive",
            "candidate:a 1 udp 1 ::1 50003 typ host",
            "candidate:a 1 udp 1 bogus 50003 typ host",
            "",
            "candidate:incomplete",
        ] {
            assert_eq!(advertised_candidate(value, ip), value);
        }
        let sdp = format!("v=0\r\na=fingerprint:sha-256 test\r\na=ice-ufrag:test\r\na={host}\r\n");
        assert_eq!(advertised_sdp(&sdp, ip), sdp.replace(host, &mapped));
        assert_eq!(advertised_sdp(&sdp, None), sdp);
    }
    #[tokio::test]
    async fn public_mapping_connects_through_udp_nat_with_encrypted_control() {
        use tokio::net::UdpSocket;
        use webrtc::data_channel::{DataChannel, DataChannelEvent};
        struct Client(mpsc::Sender<Arc<dyn DataChannel>>);
        #[async_trait::async_trait]
        impl PeerConnectionEventHandler for Client {
            async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
                let _ = self.0.try_send(channel);
            }
        }
        // Only the proxy is reachable by the client. Never give the gateway the
        // client's host candidates, so a direct private-address path cannot pass.
        let front = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = front.local_addr().unwrap().port();
        let back = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        let back_port = back.local_addr().unwrap().port();
        // Unsolicited packets must be dropped without aborting the mapping.
        front
            .send_to(b"unrelated UDP probe", ("127.0.0.1", back_port))
            .await
            .unwrap();
        let (_dir, source, _) = crate::app::tests::fixture();
        let mut config = source.config.clone();
        config.rtc.public_ip = "127.0.0.1".into();
        let app = App::node_runtime(config).unwrap();
        let (signals, mut events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(
            &app,
            1,
            port,
            signals.clone(),
            audio,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        media.offer(&signals).await.unwrap();
        let (offer, candidate) = tokio::time::timeout(Duration::from_secs(3), async {
            let (mut offer, mut candidate) = (None, None);
            loop {
                let event = events.recv().await.unwrap();
                if event["type"] == "offer" {
                    offer = Some(event["description"].clone());
                }
                if event["type"] == "ice" {
                    candidate = Some(event["candidate"].clone());
                }
                if let (Some(offer), Some(candidate)) = (&offer, &candidate) {
                    break (offer.clone(), candidate.clone());
                }
            }
        })
        .await
        .unwrap();
        let local = media
            .peer
            .local_description()
            .await
            .unwrap()
            .sdp
            .lines()
            .find(|line| line.starts_with("a=candidate:"))
            .unwrap()
            .split_ascii_whitespace()
            .nth(4)
            .unwrap()
            .to_owned();
        assert_ne!(
            local, "127.0.0.1",
            "the proxy must be separate from the real bound interface"
        );
        let target: std::net::SocketAddr = format!("{local}:{port}").parse().unwrap();
        let forwarded = Arc::new(AtomicUsize::new(0));
        let count = forwarded.clone();
        let discarded = Arc::new(AtomicUsize::new(0));
        let dropped = discarded.clone();
        let proxy = tokio::spawn(async move {
            let (mut incoming, mut outgoing) = ([0u8; 4096], [0u8; 4096]);
            let mut client = None;
            loop {
                tokio::select! {
                    packet = front.recv_from(&mut incoming) => {
                        let (n, address) = packet.unwrap();
                        if client.is_some_and(|client| client != address) {
                            dropped.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        client = Some(address);
                        back.send_to(&incoming[..n], target).await.unwrap();
                        count.fetch_add(1, Ordering::Relaxed);
                    }
                    packet = back.recv_from(&mut outgoing) => {
                        let (n, address) = packet.unwrap();
                        // Address/port-restricted NAT discards unrelated ICE probes;
                        // they must neither kill the proxy nor change its mapping.
                        if address != target {
                            dropped.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        if let Some(client) = client { front.send_to(&outgoing[..n], client).await.unwrap(); }
                    }
                }
            }
        });
        let (channels, mut opened) = mpsc::channel(2);
        let peer = PeerConnectionBuilder::new()
            .with_runtime(Arc::new(TokioRuntime))
            .with_handler(Arc::new(Client(channels)))
            .with_udp_addrs(vec!["127.0.0.1:0"])
            .build()
            .await
            .unwrap();
        peer.set_remote_description(serde_json::from_value(offer).unwrap())
            .await
            .unwrap();
        let mut answer = peer.create_answer(None).await.unwrap();
        peer.set_local_description(answer.clone()).await.unwrap();
        answer.sdp = answer
            .sdp
            .lines()
            .filter(|line| !line.starts_with("a=candidate:"))
            .collect::<Vec<_>>()
            .join("\r\n")
            + "\r\n";
        media.answer(answer).await.unwrap();
        peer.add_ice_candidate(serde_json::from_value(candidate).unwrap())
            .await
            .unwrap();
        let ready = tokio::time::timeout(Duration::from_secs(10), async {
            let channel = opened.recv().await.unwrap();
            loop {
                if let Some(DataChannelEvent::OnMessage(message)) = channel.poll().await {
                    assert_eq!(message.data.as_ref(), b"{\"type\":\"audio_control_ready\"}");
                    break;
                }
            }
            channel
                .send_text(
                    "{\"type\":\"transmit\",\"enabled\":true,\"pre_roll\":true,\"sequence\":1}",
                )
                .await
                .unwrap();
            media.control.changed().await.unwrap();
            assert!(matches!(
                media.control.borrow().as_ref(),
                Some(AudioControl::Transmit(_))
            ));
            assert!(media.check_cipher().await.unwrap().is_some());
        })
        .await;
        media.close().await;
        peer.close().await.unwrap();
        proxy.abort();
        let _ = proxy.await;
        ready.unwrap_or_else(|error| panic!("mapped host candidate must carry real bidirectional DTLS/SCTP over the NAT proxy: {error}; gateway={target}, external_port={port}, internal_port={back_port}, forwarded={}, unrelated={}", forwarded.load(Ordering::Relaxed), discarded.load(Ordering::Relaxed)));
        assert!(forwarded.load(Ordering::Relaxed) > 0);
        assert!(discarded.load(Ordering::Relaxed) > 0);
    }
    #[tokio::test]
    async fn direct_control_runs_over_real_dtls_sctp_and_closes_without_stopping_audio() {
        use webrtc::data_channel::{DataChannel, DataChannelEvent};
        struct Client {
            ice: mpsc::Sender<RTCPeerConnectionIceEvent>,
            channels: mpsc::Sender<Arc<dyn DataChannel>>,
        }
        #[async_trait::async_trait]
        impl PeerConnectionEventHandler for Client {
            async fn on_ice_candidate(&self, c: RTCPeerConnectionIceEvent) {
                let _ = self.ice.try_send(c);
            }
            async fn on_data_channel(&self, c: Arc<dyn DataChannel>) {
                let _ = self.channels.try_send(c);
            }
        }
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, mut events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let mut media = Media::new(&app, 1, 0, signals.clone(), audio, cancel.clone())
            .await
            .unwrap();
        let (ice, mut candidates) = mpsc::channel(32);
        let (channels, mut opened) = mpsc::channel(2);
        let peer = Arc::new(
            PeerConnectionBuilder::new()
                .with_runtime(Arc::new(TokioRuntime))
                .with_handler(Arc::new(Client { ice, channels }))
                .with_udp_addrs(vec!["0.0.0.0:0".to_string()])
                .build()
                .await
                .unwrap(),
        );
        media.offer(&signals).await.unwrap();
        let offer = loop {
            let e = events.recv().await.unwrap();
            if e["type"] == "offer" {
                break serde_json::from_value(e["description"].clone()).unwrap();
            }
        };
        peer.set_remote_description(offer).await.unwrap();
        let answer = peer.create_answer(None).await.unwrap();
        peer.set_local_description(answer.clone()).await.unwrap();
        media.answer(answer).await.unwrap();
        let relay = tokio::spawn({
            let remote = peer.clone();
            let local = media.peer.clone();
            async move {
                loop {
                    tokio::select! {
                        Some(e)=events.recv()=>{if e["type"]=="ice"{remote.add_ice_candidate(serde_json::from_value(e["candidate"].clone()).unwrap()).await.unwrap();}},
                        Some(e)=candidates.recv()=>{local.add_ice_candidate(e.candidate.to_json().unwrap()).await.unwrap();},
                        else=>break,
                    }
                }
            }
        });
        let channel = tokio::time::timeout(Duration::from_secs(10), opened.recv())
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(DataChannelEvent::OnMessage(m)) = channel.poll().await {
                    assert_eq!(m.data.as_ref(), b"{\"type\":\"audio_control_ready\"}");
                    break;
                }
            }
        })
        .await
        .unwrap();
        channel
            .send_text("{\"type\":\"transmit\",\"enabled\":true,\"pre_roll\":true,\"sequence\":1}")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), media.control.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            media.control.borrow_and_update().as_ref(),
            Some(AudioControl::Transmit(TransmitControl {
                enabled: true,
                sequence: 1,
                ..
            }))
        ));
        // Unknown commands revoke only this small control channel; audio and the TS actor survive.
        channel.send_text("{\"type\":\"admin\"}").await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), media.control.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            media.control.borrow_and_update().as_ref(),
            Some(AudioControl::Closed(1))
        ));
        assert!(!cancel.is_cancelled());
        for n in 0..5 {
            peer.create_data_channel(&format!("unexpected-{n}"), None)
                .await
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(3),cancel.cancelled()).await.expect("repeated unsolicited channels must revoke this peer instead of accumulating streams");
        media.close().await;
        peer.close().await.unwrap();
        relay.abort();
        assert!(cancel.is_cancelled());
    }
    #[test]
    fn direct_control_accepts_only_bounded_transmit_and_fences_old_signals() {
        let value = br#"{"type":"transmit","enabled":true,"pre_roll":true,"sequence":1}"#;
        assert!(TransmitControl::parse(value).is_some());
        for raw in [
            br#"{"type":"mute","enabled":true,"pre_roll":true,"sequence":1}"#.as_slice(),
            br#"{"type":"transmit","enabled":true,"pre_roll":true,"sequence":0}"#,
            br#"{"type":"transmit","enabled":true,"pre_roll":true,"sequence":9007199254740992}"#,
            br#"{"type":"transmit","enabled":true,"pre_roll":true,"sequence":1,"admin":true}"#,
            br#"{"type":"transmit","enabled":1,"pre_roll":true,"sequence":1}"#,
            br#"{"type":"transmit","enabled":true,"sequence":1}"#,
            &[b' '; 129],
        ] {
            assert!(TransmitControl::parse(raw).is_none());
        }
        let mut last = 0;
        assert!(control_sequence(&mut last, None)); // Older nodes/clients keep the existing WSS path.
        assert!(!control_sequence(&mut last, Some(0)));
        assert!(control_sequence(&mut last, Some(2)));
        for sequence in [None, Some(1), Some(2), Some(9_007_199_254_740_992)] {
            assert!(!control_sequence(&mut last, sequence));
        }
        assert!(control_sequence(&mut last, Some(3)));
        assert_eq!(last, 3);
    }
    #[test]
    fn queue_age_is_bounded_and_metrics_reset_without_fabricating_silence() {
        let now = Instant::now();
        let mut stats = AudioQueueStats::default();
        assert!(stats.observe(now - Duration::from_millis(7), now));
        assert!(stats.observe(now - Duration::from_millis(200), now));
        assert!(!stats.observe(now - Duration::from_millis(201), now));
        let report = stats.take(2.0);
        assert_eq!(report["received"], 3);
        assert_eq!(report["dropped"], 1);
        assert_eq!(report["p95_upper_ms"], 200);
        assert_eq!(report["mean_ms"], 103.5);
        let empty = stats.take(2.0);
        assert!(empty["mean_ms"].is_null());
        assert!(empty["p95_upper_ms"].is_null());
        assert_eq!(empty["received"], 0);
    }
    #[test]
    fn queue_p95_buckets_are_true_upper_bounds_at_submillisecond_edges() {
        let now = Instant::now();
        for (nanos, upper) in [
            (0, 0),
            (1, 10),
            (900_000, 10),
            (10_000_000, 10),
            (10_900_000, 20),
            (199_999_999, 200),
        ] {
            let mut stats = AudioQueueStats::default();
            assert!(stats.observe(now - Duration::from_nanos(nanos), now));
            assert_eq!(stats.take(2.0)["p95_upper_ms"], upper);
        }
    }
    #[tokio::test]
    async fn speech_boundaries_compress_idle_ids_but_preserve_active_packet_loss() {
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, _events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(&app, 1, 0, signals, audio, CancellationToken::new())
            .await
            .unwrap();
        media
            .sync_speakers(&[2], &media.signals.clone())
            .await
            .unwrap();
        media.secure.store(true, Ordering::Release);
        media.speakers.get_mut(&2).unwrap().ready = true;
        let frame = &[0xf8, 0xff, 0xfe];
        assert!(media.audio(2, 65530, frame).await.unwrap());
        assert!(media.audio(2, 65532, frame).await.unwrap());
        assert_eq!(
            65532u16.wrapping_sub(media.speakers[&2].sequence_offset),
            65532,
            "a missing frame inside active speech remains an RTP sequence gap"
        );
        assert!(media.audio(2, 65533, &[]).await.unwrap());
        assert!(media.audio(2, 16, frame).await.unwrap());
        assert_eq!(
            16u16.wrapping_sub(media.speakers[&2].sequence_offset),
            65533,
            "native idle voice IDs, including wrap, are not lost RTP packets"
        );
        assert!(media.audio(2, 17, frame).await.unwrap());
        assert!(media.audio(2, 19, frame).await.unwrap());
        assert_eq!(19u16.wrapping_sub(media.speakers[&2].sequence_offset), 0);
        // A delayed end marker can itself carry idle IDs.
        assert!(media.audio(2, 40, &[0xff]).await.unwrap());
        assert!(media.audio(2, 41, frame).await.unwrap());
        assert_eq!(41u16.wrapping_sub(media.speakers[&2].sequence_offset), 1);
        // Recover the boundary when the UDP end marker was lost.
        media.speakers.get_mut(&2).unwrap().last -= Duration::from_millis(300);
        media.speakers.get_mut(&2).unwrap().clock -= Duration::from_millis(300);
        assert!(media.audio(2, 60, frame).await.unwrap());
        assert_eq!(60u16.wrapping_sub(media.speakers[&2].sequence_offset), 2);
        assert!(!media.audio(2, 59, frame).await.unwrap());
        media.close().await;
    }
    #[tokio::test]
    async fn speech_bursts_do_not_add_their_sample_duration_again_at_restart() {
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, _events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(&app, 1, 0, signals, audio, CancellationToken::new())
            .await
            .unwrap();
        media
            .sync_speakers(&[2], &media.signals.clone())
            .await
            .unwrap();
        media.secure.store(true, Ordering::Release);
        media.speakers.get_mut(&2).unwrap().ready = true;
        assert!(media.audio(2, 100, &[0xf8, 0xff, 0xfe]).await.unwrap());
        let start = media.speakers[&2].clock;
        // A speech lead-in arrives together, but still represents 120ms of PCM.
        for sequence in 101..=106 {
            assert!(media.audio(2, sequence, &[0xf8, 0xff, 0xfe]).await.unwrap());
        }
        assert_eq!(media.speakers[&2].timestamp, 5760);
        assert!(media.audio(2, 107, &[]).await.unwrap());
        // Advance the wall clock by 400ms without waiting in a protocol test.
        media.speakers.get_mut(&2).unwrap().last -= Duration::from_millis(400);
        media.speakers.get_mut(&2).unwrap().clock -= Duration::from_millis(400);
        let minimum =
            ((start.elapsed() + Duration::from_millis(400)).as_nanos() * 48 / 1_000_000) as u32;
        assert!(media.audio(2, 108, &[0xf8, 0xff, 0xfe]).await.unwrap());
        let maximum = ((start.elapsed() + Duration::from_millis(400)).as_nanos() * 48 / 1_000_000)
            as u32
            + 48;
        let resumed = media.speakers[&2].timestamp;
        media.close().await;
        assert!(
            (minimum.saturating_sub(48)..=maximum).contains(&resumed),
            "burst duration must not be counted twice: got {resumed}, expected {minimum}..={maximum}"
        );
    }
    #[tokio::test]
    async fn end_markers_do_not_rebase_the_last_audio_sample_clock() {
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, _events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(&app, 1, 0, signals, audio, CancellationToken::new())
            .await
            .unwrap();
        media
            .sync_speakers(&[2], &media.signals.clone())
            .await
            .unwrap();
        media.secure.store(true, Ordering::Release);
        media.speakers.get_mut(&2).unwrap().ready = true;
        assert!(media.audio(2, 100, &[0xf8, 0xff, 0xfe]).await.unwrap());
        let timestamp = media.speakers[&2].timestamp;
        // Model an end marker delivered well after its last 20ms audio frame.
        let anchor = Instant::now() - Duration::from_millis(120);
        media.speakers.get_mut(&2).unwrap().last = anchor;
        media.speakers.get_mut(&2).unwrap().clock = anchor;
        assert!(media.audio(2, 101, &[]).await.unwrap());
        let end_timestamp = media.speakers[&2].timestamp;
        let end_anchor = media.speakers[&2].last;
        let before = (anchor.elapsed().as_nanos() * 48 / 1_000_000) as u32;
        assert!(media.audio(2, 102, &[0xf8, 0xff, 0xfe]).await.unwrap());
        let after = (anchor.elapsed().as_nanos() * 48 / 1_000_000) as u32;
        let resumed = media.speakers[&2].timestamp.wrapping_sub(timestamp);
        for i in 0..8u16 {
            let timestamp = media.speakers[&2].timestamp;
            let anchor = media.speakers[&2].last;
            let marker: &[u8] = if i % 2 == 0 { &[] } else { &[0] };
            assert!(media.audio(2, 103 + i * 2, marker).await.unwrap());
            assert_eq!(media.speakers[&2].timestamp, timestamp);
            assert_eq!(media.speakers[&2].last, anchor);
            assert!(
                media
                    .audio(2, 104 + i * 2, &[0xf8, 0xff, 0xfe])
                    .await
                    .unwrap()
            );
            assert!(
                media.speakers[&2].timestamp.wrapping_sub(timestamp) >= 960,
                "adjacent speech frames must not overlap when their end marker is immediate"
            );
        }
        media.close().await;
        assert_eq!(
            end_timestamp, timestamp,
            "a non-audio marker has no sample duration"
        );
        assert_eq!(
            end_anchor, anchor,
            "late end markers must not erase elapsed silence"
        );
        assert!(
            (before..=after).contains(&resumed),
            "resume must use the last audio anchor, got {resumed} samples; expected {before}..={after}"
        );
    }
    #[tokio::test]
    async fn speech_restarts_preserve_short_silence_on_the_rtp_clock() {
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, _events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(&app, 1, 0, signals, audio, CancellationToken::new())
            .await
            .unwrap();
        media
            .sync_speakers(&[2], &media.signals.clone())
            .await
            .unwrap();
        // Clock conversion only: real cipher negotiation is covered by the RTC tests.
        media.secure.store(true, Ordering::Release);
        media.speakers.get_mut(&2).unwrap().ready = true;
        assert!(media.audio(2, 100, &[0xf8, 0xff, 0xfe]).await.unwrap());
        assert!(media.audio(2, 101, &[]).await.unwrap());
        let boundary = media.speakers[&2].timestamp;
        media.speakers.get_mut(&2).unwrap().last = Instant::now() - Duration::from_millis(120);
        media.speakers.get_mut(&2).unwrap().clock = media.speakers[&2].last;
        assert!(media.audio(2, 102, &[0xf8, 0xff, 0xfe]).await.unwrap());
        let resumed = media.speakers[&2].timestamp;
        let offset = media.speakers[&2].sequence_offset;
        media.close().await;
        assert!(
            resumed.wrapping_sub(boundary) >= 5760,
            "120ms speech pause must not collapse to a 20ms RTP step: {} samples",
            resumed.wrapping_sub(boundary)
        );
        assert_eq!(offset, 1, "TS end marker is not an RTP packet loss");
    }
    #[tokio::test]
    async fn speech_clock_wraps_after_more_than_one_timestamp_period_of_silence() {
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, _events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(&app, 1, 0, signals, audio, CancellationToken::new())
            .await
            .unwrap();
        media
            .sync_speakers(&[2], &media.signals.clone())
            .await
            .unwrap();
        media.secure.store(true, Ordering::Release);
        media.speakers.get_mut(&2).unwrap().ready = true;
        assert!(media.audio(2, 1, &[0xf8, 0xff, 0xfe]).await.unwrap());
        assert!(media.audio(2, 2, &[]).await.unwrap());
        let boundary = media.speakers[&2].timestamp;
        let at = Instant::now() - Duration::from_secs(26 * 3600);
        media.speakers.get_mut(&2).unwrap().last = at;
        media.speakers.get_mut(&2).unwrap().clock = at;
        let minimum = (at.elapsed().as_nanos() * 48 / 1_000_000) as u32;
        assert!(media.audio(2, 3, &[0xf8, 0xff, 0xfe]).await.unwrap());
        let maximum = (at.elapsed().as_nanos() * 48 / 1_000_000) as u32;
        let advance = media.speakers[&2].timestamp.wrapping_sub(boundary);
        media.close().await;
        assert!((minimum..=maximum).contains(&advance));
    }
    #[tokio::test]
    async fn reordered_audio_is_forwarded_once_without_rewinding_the_source_clock() {
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, _events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(&app, 1, 0, signals, audio, CancellationToken::new())
            .await
            .unwrap();
        media
            .sync_speakers(&[2], &media.signals.clone())
            .await
            .unwrap();
        media.secure.store(true, Ordering::Release);
        media.speakers.get_mut(&2).unwrap().ready = true;
        let frame = &[0xf8, 0xff, 0xfe];
        assert!(media.audio(2, u16::MAX - 1, frame).await.unwrap());
        assert!(media.audio(2, 0, frame).await.unwrap());
        let timestamp = media.speakers[&2].timestamp;
        let last = media.speakers[&2].last;
        let reordered = media.audio(2, u16::MAX, frame).await.unwrap();
        assert_eq!(media.speakers[&2].timestamp, timestamp);
        assert_eq!(media.speakers[&2].last, last);
        assert_eq!(media.speakers[&2].seq, Some(0));
        let duplicate = media.audio(2, u16::MAX, frame).await.unwrap();
        let stale = media.audio(2, u16::MAX - 2, frame).await.unwrap();
        assert!(media.audio(2, 1, &[]).await.unwrap());
        let after_end = media.audio(2, u16::MAX, frame).await.unwrap();
        assert!(media.audio(2, 2, frame).await.unwrap());
        assert!(!media.audio(2, 0, frame).await.unwrap()); // Previous speech epoch.
        assert!(media.audio(2, 65, frame).await.unwrap());
        assert!(media.audio(2, 3, frame).await.unwrap()); // Missing frame inside the window.
        assert!(!media.audio(2, 2, frame).await.unwrap()); // Already received, 63 behind.
        assert!(media.audio(2, 66, frame).await.unwrap());
        assert!(!media.audio(2, 2, frame).await.unwrap()); // Outside the 64-bit window.
        assert!(media.audio(2, 67, &[0x18, 0]).await.unwrap());
        assert!(!media.audio(2, 64, frame).await.unwrap()); // Changed packet duration.
        assert!(media.audio(2, 69, frame).await.unwrap());
        assert!(media.audio(2, 71, frame).await.unwrap());
        media.speakers.get_mut(&2).unwrap().last = Instant::now() - Duration::from_millis(201);
        media.speakers.get_mut(&2).unwrap().clock = media.speakers[&2].last;
        assert!(!media.audio(2, 70, frame).await.unwrap()); // Expired speech epoch.
        media.close().await;
        assert!(
            reordered,
            "a valid reordered frame must reach the browser jitter buffer"
        );
        assert!(!duplicate && !stale && !after_end);
    }
    #[tokio::test]
    async fn idle_audio_clock_resynchronizes_without_a_received_end_marker() {
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, _events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(&app, 1, 0, signals, audio, CancellationToken::new())
            .await
            .unwrap();
        media
            .sync_speakers(&[2], &media.signals.clone())
            .await
            .unwrap();
        media.secure.store(true, Ordering::Release);
        media.speakers.get_mut(&2).unwrap().ready = true;
        assert!(media.audio(2, 1, &[0xf8, 0xff, 0xfe]).await.unwrap());
        let start = media.speakers[&2].timestamp;
        assert!(media.audio(2, 2, &[0xf8, 0xff, 0xfe]).await.unwrap());
        assert_eq!(media.speakers[&2].timestamp.wrapping_sub(start), 960);
        let start = media.speakers[&2].timestamp;
        media.speakers.get_mut(&2).unwrap().last = Instant::now() - Duration::from_millis(300);
        media.speakers.get_mut(&2).unwrap().clock = media.speakers[&2].last;
        assert!(media.audio(2, 3, &[0xf8, 0xff, 0xfe]).await.unwrap());
        let advance = media.speakers[&2].timestamp.wrapping_sub(start);
        media.close().await;
        assert!(
            advance >= 14400,
            "missing UDP end markers must not disable idle clock recovery"
        );
    }
    #[tokio::test]
    async fn speech_clock_preserves_loss_wrap_and_rejects_stale_or_insecure_packets() {
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, _events) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let mut media = Media::new(&app, 1, 0, signals, audio, CancellationToken::new())
            .await
            .unwrap();
        media
            .sync_speakers(&[2], &media.signals.clone())
            .await
            .unwrap();
        media.secure.store(true, Ordering::Release);
        media.speakers.get_mut(&2).unwrap().ready = true;
        assert!(
            media
                .audio(2, u16::MAX - 1, &[0xf8, 0xff, 0xfe])
                .await
                .unwrap()
        );
        media.speakers.get_mut(&2).unwrap().timestamp = u32::MAX - 400;
        assert!(media.audio(2, 0, &[0x18, 0]).await.unwrap()); // Lost 20ms frame and sequence wrap.
        assert_eq!(media.speakers[&2].timestamp, 1519);
        assert!(media.audio(2, 1, &[0]).await.unwrap()); // Single-byte TS end marker after 60ms audio.
        assert_eq!(media.speakers[&2].timestamp, 1519);
        let last = media.speakers[&2].last;
        assert!(!media.audio(2, 1, &[]).await.unwrap());
        assert!(!media.audio(2, 0, &[0xf8, 0xff, 0xfe]).await.unwrap());
        assert!(!media.audio(2, 2, &[0xfb, 0]).await.unwrap());
        assert_eq!(media.speakers[&2].last, last);
        assert_eq!(media.speakers[&2].sequence_offset, 1);
        assert!(media.speakers[&2].ended);
        media.speakers.get_mut(&2).unwrap().last = Instant::now() - Duration::from_millis(120);
        media.speakers.get_mut(&2).unwrap().clock = media.speakers[&2].last;
        assert!(media.audio(2, 2, &[0xf8, 0xff, 0xfe]).await.unwrap());
        assert!(media.speakers[&2].timestamp.wrapping_sub(1519) >= 5760);
        assert!(!media.speakers[&2].ended);
        let resumed = media.speakers[&2].timestamp;
        media.secure.store(false, Ordering::Release);
        assert!(!media.audio(2, 3, &[0xf8, 0xff, 0xfe]).await.unwrap());
        assert_eq!(media.speakers[&2].timestamp, resumed);
        media.close().await;
    }
    #[tokio::test]
    async fn malformed_received_audio_does_not_close_the_media_connection() {
        let (_dir, app, _) = crate::app::tests::fixture();
        let (signals, _) = mpsc::channel(64);
        let (audio, _) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let mut media = Media::new(&app, 1, 0, signals, audio, cancel.clone())
            .await
            .unwrap();
        media.secure.store(true, Ordering::Release);
        let mut valid = true;
        for packet in [&[0xff, 0][..], &[0xfb, 0], &[0xfb, 7]] {
            valid &= media.audio(2, 1, packet).await.is_ok();
        }
        let stayed_secure = media.secure.load(Ordering::Acquire);
        let stayed_active = !cancel.is_cancelled();
        media.close().await;
        assert!(
            valid,
            "bad Opus frames must be dropped without tearing down a recipient's connection"
        );
        assert!(stayed_secure && stayed_active);
        assert!(cancel.is_cancelled() && !media.secure.load(Ordering::Acquire));
    }
    #[test]
    fn automatic_speech_retains_leading_packets_and_discards_expired_or_muted_audio() {
        let now = Instant::now();
        let mut lead = SpeechLead::default();
        for i in 0..15 {
            lead.push(
                Bytes::from(vec![0x98, i]),
                now + Duration::from_millis(u64::from(i) * 20),
            );
        }
        let captured = lead.take(now + Duration::from_millis(300));
        assert_eq!(captured.len(), 10);
        assert_eq!(
            captured[0][1], 5,
            "retain the first phoneme from before the delayed start signal"
        );
        assert_eq!(captured[9][1], 14);
        assert!(
            lead.take(now + Duration::from_millis(300)).is_empty(),
            "duplicate start must not replay speech twice"
        );
        lead.push(Bytes::from_static(&[0x98, 42]), now);
        assert!(lead.take(now + Duration::from_millis(201)).is_empty());
        lead.push(Bytes::from_static(&[0x98, 42]), now);
        lead.clear();
        assert!(
            lead.take(now).is_empty(),
            "mute/PTT/whisper/channel changes discard previous speech"
        );
        for _ in 0..1000 {
            lead.push(Bytes::from(vec![0x98; 1276]), now);
        }
        assert!(
            lead.take(now).len() <= 10,
            "packet flood cannot grow the duration/byte bound"
        );
    }
    #[tokio::test]
    async fn transient_ice_disconnect_does_not_cancel_the_connection() {
        let (signals, mut events) = mpsc::channel(8);
        let (audio, _) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let handler = Handler {
            signals,
            audio,
            secure: Arc::new(AtomicBool::new(true)),
            cancel: cancel.clone(),
            unexpected_channels: AtomicUsize::new(0),
            public_ip: None,
        };
        handler
            .on_connection_state_change(RTCPeerConnectionState::Disconnected)
            .await;
        assert!(
            !cancel.is_cancelled(),
            "temporary ICE loss must not behave like account revocation"
        );
        assert!(!handler.secure.load(Ordering::Acquire));
        assert_eq!(events.recv().await.unwrap()["type"], "rtc_state");
        handler
            .on_connection_state_change(RTCPeerConnectionState::Connected)
            .await;
        assert!(!cancel.is_cancelled());
        assert!(
            !handler.secure.load(Ordering::Acquire),
            "cipher must be verified again before voice resumes"
        );
        handler
            .on_connection_state_change(RTCPeerConnectionState::Failed)
            .await;
        assert!(cancel.is_cancelled(), "failed ICE needs a new transport");
    }
    #[test]
    fn dtx_headers_do_not_light_speech_but_short_audio_is_preserved() {
        for packet in [&[0xf8][..], &[0xf9], &[0xfa, 0], &[0xfb, 1], &[0xfb, 3]] {
            assert!(!opus_has_audio(packet));
            assert!(opus_samples(packet).is_ok());
        }
        for packet in [&[0xf8, 1][..], &[0xf8, 0xff, 0xfe], &[0xfb, 1, 1]] {
            assert!(opus_has_audio(packet));
        }
    }
    #[test]
    fn ice_and_embedded_sdp_do_not_probe_private_or_named_targets() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "::1",
            "::ffff:127.0.0.1",
            "fd00::1",
            "router.local",
        ] {
            let candidate = format!("candidate:1 1 udp 123 {address} 9000 typ host");
            assert!(!public_candidate(&candidate));
            for space in ["", " ", "\t", "\u{a0}"] {
                let sdp = format!("v=0\na={space}{candidate}\na=sendrecv\n");
                let filtered = public_sdp(&sdp);
                assert!(!filtered.contains(address));
                assert!(filtered.contains("a=sendrecv"));
            }
        }
        let candidate = "candidate:1 1 udp 123 8.8.8.8 9000 typ srflx raddr 10.0.0.2 rport 4000";
        assert!(public_candidate(candidate));
        assert!(public_sdp(&format!("a={candidate}\r\n")).contains(candidate));
    }
    #[test]
    fn opus_duration_checks_bound_forwarded_frames() {
        assert_eq!(opus_samples(&[0xf8]).unwrap(), 960);
        assert_eq!(opus_samples(&[0xfc]).unwrap(), 960);
        assert_eq!(opus_samples(&[0xfb, 3]).unwrap(), 2880);
        assert!(opus_samples(&[]).is_err());
        assert!(opus_samples(&[0xff]).is_err());
        assert!(opus_samples(&[0xfb, 0]).is_err());
        assert!(opus_samples(&[0xfb, 7]).is_err());
    }
}
