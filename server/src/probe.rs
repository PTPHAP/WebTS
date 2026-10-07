//! Local protocol acceptance tool. Never exposed as a website endpoint.
use anyhow::{Context, Result, bail};
use futures::StreamExt;
use serde::Serialize;
use std::time::Duration;
use tsclientlib::{Connection, DisconnectOptions, Identity, StreamItem};
use tsproto_packets::packets::Flags;
use tsproto_types::CodecEncryptionMode;

#[derive(Serialize)]
pub struct Report {
    pub uid: String,
    pub connected: bool,
    pub forced_encryption: bool,
    pub ts_cipher: &'static str,
    pub received_voice_packets: u64,
    pub received_whisper_packets: u64,
    pub native_voice_round_trip_verified: bool,
}

pub async fn run(address: String, identity: Identity, seconds: u64) -> Result<Report> {
    let mut report = Report {
        uid: crate::identity::uid(&identity),
        connected: false,
        forced_encryption: false,
        ts_cipher: "AES-128-EAX (64-bit authentication tag)",
        received_voice_packets: 0,
        received_whisper_packets: 0,
        native_voice_round_trip_verified: false,
    };
    let mut connection = Connection::build(address)
        .identity(identity)
        .name("WebTS protocol probe")
        .log_commands(false)
        .log_packets(false)
        .log_udp_packets(false)
        .connect()?;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = connection.events().next().await.context("连接已关闭")??;
            if matches!(event, StreamItem::BookEvents(_)) {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("TeamSpeak 握手超时")??;
    report.connected = true;
    report.forced_encryption =
        connection.get_state()?.server.codec_encryption_mode == CodecEncryptionMode::ForcedOn;
    if !report.forced_encryption {
        bail!("服务器未启用全局语音加密，拒绝语音验证");
    }
    let deadline = tokio::time::sleep(Duration::from_secs(seconds.min(300)));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = async { connection.events().next().await } => {
                let event = event.context("连接已关闭")??;
                if connection.get_state()?.server.codec_encryption_mode != CodecEncryptionMode::ForcedOn {
                    bail!("全局语音加密策略已改变，停止验证");
                }
                if let StreamItem::Audio(packet) = event {
                    if packet.data().packet().header().flags().contains(Flags::UNENCRYPTED) {
                        bail!("收到未加密语音，停止验证");
                    }
                    if packet.data().packet().header().packet_type() == tsproto_packets::packets::PacketType::VoiceWhisper {
                        report.received_whisper_packets += 1;
                    } else { report.received_voice_packets += 1; }
                }
            }
        }
    }
    connection.disconnect(DisconnectOptions::new())?;
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        while connection.events().next().await.is_some() {}
    })
    .await;
    Ok(report)
}
