use crate::app::{Api, App};
use anyhow::{Result, bail};
use axum::{Json, extract::State, http::HeaderMap};
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
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use webrtc::{
    media_stream::{
        track_local::{TrackLocal, static_rtp::TrackLocalStaticRTP},
        track_remote::{TrackRemote, TrackRemoteEvent},
    },
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCBundlePolicy,
        RTCConfigurationBuilder, RTCIceCandidateType, RTCIceServer, RTCPeerConnectionIceEvent,
        RTCPeerConnectionState, RTCSessionDescription, RTCStatsReportEntry, SettingEngineBuilder,
        SrtpProtectionProfile, StatsSelector,
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
pub async fn ice_config(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    let owner = app.session(&headers)?.user.id;
    Ok(Json(
        json!({"iceServers":ice_servers(&app,owner)?.iter().map(|s|json!({"urls":s.urls,"username":s.username,"credential":s.credential})).collect::<Vec<_>>()}),
    ))
}
struct Handler {
    signals: mpsc::Sender<Value>,
    audio: mpsc::Sender<Packet>,
    secure: Arc<AtomicBool>,
    cancel: CancellationToken,
}
#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        if let Ok(candidate) = event.candidate.to_json() {
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
                        && secure.load(Ordering::Acquire)&&packet.payload.len()<=1276&&opus_samples(&packet.payload).is_ok(){let _=tx.try_send(packet);}
                }}
            }
        });
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
    last: Instant,
}
pub struct Media {
    pub peer: Arc<dyn PeerConnection>,
    pub secure: Arc<AtomicBool>,
    speakers: HashMap<u16, Speaker>,
    pub negotiating: bool,
    pub dirty: bool,
    local_ice: bool,
}
impl Media {
    pub async fn new(
        app: &App,
        owner: i64,
        port: u16,
        signals: mpsc::Sender<Value>,
        audio: mpsc::Sender<Packet>,
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
        let mut settings = SettingEngineBuilder::new().with_srtp_protection_profiles(vec![
            SrtpProtectionProfile::Srtp_Aead_Aes_256_Gcm,
            SrtpProtectionProfile::Srtp_Aead_Aes_128_Gcm,
        ]);
        if !app.config.rtc.public_ip.is_empty() {
            settings = settings.with_nat_1to1_ips(
                vec![app.config.rtc.public_ip.clone()],
                RTCIceCandidateType::Host,
            );
        }
        let secure = Arc::new(AtomicBool::new(false));
        let peer = PeerConnectionBuilder::new()
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
                signals,
                audio,
                secure: secure.clone(),
                cancel,
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
        Ok(Self {
            peer: Arc::new(peer),
            secure,
            speakers: HashMap::new(),
            negotiating: false,
            dirty: true,
            local_ice: app.config.allow_insecure_localhost
                && app.config.public_url.starts_with("http://localhost:"),
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
                    last: Instant::now(),
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
        let offer = self.peer.create_offer(None).await?;
        self.peer.set_local_description(offer.clone()).await?;
        signals
            .try_send(json!({"type":"offer","description":offer}))
            .map_err(|_| anyhow::anyhow!("信令队列已满"))?;
        self.negotiating = true;
        self.dirty = false;
        Ok(())
    }
    pub async fn answer(&mut self, mut answer: RTCSessionDescription) -> Result<()> {
        if !self.local_ice {
            answer.sdp = public_sdp(&answer.sdp);
        }
        self.peer.set_remote_description(answer).await?;
        self.negotiating = false;
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
    pub async fn audio(&mut self, from: u16, sequence: u16, data: &[u8]) -> Result<()> {
        if !self.secure.load(Ordering::Acquire) {
            return Ok(());
        }
        // TS uses both empty and single-byte packets to end a speech stream.
        // This boundary is TS-specific; browser RTP Opus keeps its own validation.
        let samples = if data.len() <= 1 {
            None
        } else {
            Some(opus_samples(data)?)
        };
        let Some(s) = self.speakers.get_mut(&from) else {
            return Ok(());
        };
        if let Some(previous) = s.seq {
            let delta = sequence.wrapping_sub(previous);
            if delta == 0 || delta >= 0x8000 {
                return Ok(());
            }
            let elapsed = s.last.elapsed();
            let advance = if elapsed > Duration::from_millis(200) {
                (elapsed.as_secs_f64() * 48000.0) as u32
            } else {
                u32::from(delta) * s.samples
            };
            s.timestamp = s.timestamp.wrapping_add(advance);
        }
        s.seq = Some(sequence);
        s.last = Instant::now();
        let Some(samples) = samples else {
            // TS end markers occupy a TS sequence number but have no RTP audio.
            // Compress that intentional gap so it is not reported as packet loss.
            s.sequence_offset = s.sequence_offset.wrapping_add(1);
            return Ok(());
        };
        s.samples = samples;
        let packet = Packet {
            header: Header {
                version: 2,
                payload_type: 111,
                sequence_number: sequence.wrapping_sub(s.sequence_offset),
                timestamp: s.timestamp,
                ssrc: s.ssrc,
                ..Default::default()
            },
            payload: Bytes::copy_from_slice(data),
        };
        let _ = tokio::time::timeout(Duration::from_millis(20), s.track.write_rtp(packet)).await;
        Ok(())
    }
    pub async fn close(&self) {
        self.secure.store(false, Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_secs(2), self.peer.close()).await;
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
    async fn transient_ice_disconnect_does_not_cancel_the_connection() {
        let (signals, mut events) = mpsc::channel(8);
        let (audio, _) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let handler = Handler {
            signals,
            audio,
            secure: Arc::new(AtomicBool::new(true)),
            cancel: cancel.clone(),
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
