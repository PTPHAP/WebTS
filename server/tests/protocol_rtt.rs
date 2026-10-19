//! Encrypted protocol loopback regressions; not physical audio latency acceptance.
use futures::{FutureExt, StreamExt};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tsproto::connection::{ConnectedParams, Connection, Event, StreamItem};
use tsproto::resend::ResenderState;
use tsproto_packets::packets::{Direction, Flags, OutCommand, PacketType};
use tsproto_types::crypto::EccKeyPrivP256;

#[tokio::test]
async fn keepalive_is_sent_in_the_poll_that_creates_it() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let sink = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut client = Connection::new(true, sink.local_addr().unwrap(), Box::new(socket));
    client.params = Some(ConnectedParams::new(
        EccKeyPrivP256::create().to_pub(),
        [0; 64],
        [0x42; 8],
        64,
    ));
    client.resender.set_state(ResenderState::Connected);
    let queued = Arc::new(AtomicUsize::new(0));
    let sent = Arc::new(AtomicUsize::new(0));
    let (q, s) = (queued.clone(), sent.clone());
    client
        .event_listeners
        .push(Box::new(move |event| match event {
            Event::SendPacket(packet) if packet.header().packet_type() == PacketType::Ping => {
                q.fetch_add(1, Ordering::Relaxed);
            }
            Event::SendUdpPacket(packet) if packet.packet_type() == PacketType::Ping => {
                s.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }));
    // Shift the keepalive deadline away from the 1s statistics deadline.
    tokio::time::sleep(Duration::from_millis(250)).await;
    client.resender.received_packet();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(matches!(
        client.next().now_or_never(),
        Some(Some(Ok(StreamItem::NetworkStatsUpdated)))
    ));
    tokio::time::sleep(Duration::from_millis(230)).await;
    let _ = client.next().now_or_never();
    assert_eq!(
        queued.load(Ordering::Relaxed),
        1,
        "probe must reach the real keepalive path"
    );
    assert_eq!(
        sent.load(Ordering::Relaxed),
        1,
        "keepalive must not wait for the actor's next 100ms maintenance tick"
    );
}

#[tokio::test]
async fn command_rtt_excludes_time_waiting_to_be_sent() {
    command_sample(160, 0).await;
}

#[tokio::test]
async fn command_rtt_preserves_peer_response_delay() {
    let reported = command_sample(0, 80).await;
    assert!(
        reported >= Duration::from_millis(35),
        "real peer delay must remain in RTT"
    );
}

async fn command_sample(queued_ms: u64, peer_delay_ms: u64) -> Duration {
    let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut client = Connection::new(true, b.local_addr().unwrap(), Box::new(a));
    let mut server = Connection::new(false, client.udp_socket.local_addr().unwrap(), Box::new(b));
    for con in [&mut client, &mut server] {
        con.params = Some(ConnectedParams::new(
            EccKeyPrivP256::create().to_pub(),
            [0; 64],
            [0x42; 8],
            64,
        ));
        con.params.as_mut().unwrap().c_id = 1;
        con.resender.set_state(ResenderState::Connected);
    }
    let sent = Arc::new(Mutex::new(None));
    let sample = sent.clone();
    client.event_listeners.push(Box::new(move |event| {
        if let Event::SendUdpPacket(packet) = event
            && packet.packet_type() == PacketType::Command
        {
            assert!(!packet.data().header().flags().contains(Flags::UNENCRYPTED));
            *sample.lock().unwrap() = Some(Instant::now());
        }
    }));
    client
        .send_packet(
            OutCommand::new(
                Direction::C2S,
                Flags::empty(),
                PacketType::Command,
                "rttprobe",
            )
            .into_packet(),
        )
        .unwrap();
    // A command can wait behind negotiation/work before the actor polls the socket.
    tokio::time::sleep(Duration::from_millis(queued_ms)).await;
    let _ = client.next().now_or_never();
    assert!(
        sent.lock().unwrap().is_some(),
        "command must really leave the socket"
    );
    tokio::time::sleep(Duration::from_millis(peer_delay_ms)).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            tokio::select! {
                event = client.next() => {
                    if matches!(event.unwrap().unwrap(), StreamItem::AckPacket(_)) { break; }
                }
                event = server.next() => { event.unwrap().unwrap(); }
            }
        }
    })
    .await
    .unwrap();
    let wire = sent.lock().unwrap().unwrap().elapsed();
    let reported = client.resender.stats.rtt;
    eprintln!("encrypted loopback: send-to-ACK={wire:?}, reported half-weight RTT={reported:?}");
    assert!(
        reported <= wire / 2 + Duration::from_millis(20),
        "local send queue time was included in network RTT"
    );
    assert!(!reported.is_zero(), "RTT must be measured, not zeroed");
    reported
}
