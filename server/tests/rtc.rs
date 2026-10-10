use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use web_ts::{app::App, config::Config, media::Media};

// Two real UDP/DTLS/SRTP peers, without a TeamSpeak server or fake crypto stats.
#[tokio::test]
async fn encrypted_opus_packet_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    std::fs::write(&key, hex::encode([9; 32])).unwrap();
    let config: Config = toml::from_str(include_str!("../../config.example.toml")).unwrap();
    let app = App::new(Config {
        database: dir.path().join("db").to_string_lossy().into_owned(),
        master_key_file: key.to_string_lossy().into_owned(),
        ..config
    })
    .unwrap();
    let (at, mut ar) = mpsc::channel(64);
    let (bt, mut br) = mpsc::channel(64);
    let (aa, mut a_audio) = mpsc::channel(8);
    let (ba, mut b_audio) = mpsc::channel(8);
    let mut a = Media::new(&app, 1, 0, at.clone(), aa, CancellationToken::new())
        .await
        .unwrap();
    let mut b = Media::new(&app, 2, 0, bt.clone(), ba, CancellationToken::new())
        .await
        .unwrap();
    a.sync_speakers(&[7], &at).await.unwrap();
    b.sync_speakers(&[8], &bt).await.unwrap();
    // Exercise the gateway's offer/answer path, including its track readiness.
    // Negotiate both sending peers rather than bypassing Media's bookkeeping.
    async fn negotiate(
        sender: &mut Media,
        receiver: &mut Media,
        events: &mut mpsc::Receiver<serde_json::Value>,
        signals: &mpsc::Sender<serde_json::Value>,
    ) {
        sender.offer(signals).await.unwrap();
        let mut candidates = Vec::new();
        let offer: webrtc::peer_connection::RTCSessionDescription = loop {
            let event = events.recv().await.unwrap();
            if event["type"] == "offer" {
                break serde_json::from_value(event["description"].clone()).unwrap();
            }
            if event["type"] == "ice" {
                candidates.push(event["candidate"].clone());
            }
        };
        assert!(offer.sdp.contains("usedtx=1"));
        receiver.peer.set_remote_description(offer).await.unwrap();
        for candidate in candidates {
            receiver
                .peer
                .add_ice_candidate(serde_json::from_value(candidate).unwrap())
                .await
                .unwrap();
        }
        let answer = receiver.peer.create_answer(None).await.unwrap();
        receiver
            .peer
            .set_local_description(answer.clone())
            .await
            .unwrap();
        sender.answer(answer).await.unwrap();
    }
    negotiate(&mut a, &mut b, &mut ar, &at).await;
    negotiate(&mut b, &mut a, &mut br, &bt).await;
    let deadline = tokio::time::sleep(Duration::from_secs(15));
    tokio::pin!(deadline);
    let mut timer = tokio::time::interval(Duration::from_millis(50));
    let payload = [0xf8, 0xff, 0xfe];
    let mut seq = 0;
    let (mut got_a, mut got_b) = (false, false);
    loop {
        tokio::select! {
            _=&mut deadline=>panic!("UDP DTLS-SRTP round trip timed out"),
            Some(event)=ar.recv()=>{if event["type"]=="ice"{b.peer.add_ice_candidate(serde_json::from_value(event["candidate"].clone()).unwrap()).await.unwrap();}},
            Some(event)=br.recv()=>{if event["type"]=="ice"{a.peer.add_ice_candidate(serde_json::from_value(event["candidate"].clone()).unwrap()).await.unwrap();}},
            _=timer.tick()=>{let ac=a.check_cipher().await.unwrap();let bc=b.check_cipher().await.unwrap();if let(Some(ac),Some(bc))=(ac,bc){assert_eq!(ac["browser"],"SRTP_AEAD_AES_256_GCM");assert_eq!(bc["browser"],"SRTP_AEAD_AES_256_GCM");a.audio(7,seq,&payload).await.unwrap();b.audio(8,seq,&payload).await.unwrap();seq+=1;}},
            Some(packet)=a_audio.recv()=>{assert_eq!(packet.payload.as_ref(),payload);got_a=true;if got_b{break;}},
            Some(packet)=b_audio.recv()=>{assert_eq!(packet.payload.as_ref(),payload);got_b=true;if got_a{break;}},
        }
    }
    a.audio(7, seq, &[])
        .await
        .expect("TS end-of-speech marker must not fail the RTC bridge");
    // Repeated/stale end markers must neither disconnect nor double-compress RTP.
    a.audio(7, seq, &[]).await.unwrap();
    a.audio(7, seq.wrapping_add(1), &[0xff])
        .await
        .expect("native TS single-byte end markers must not be parsed as Opus");
    a.audio(7, seq.wrapping_add(2), &[0x03]).await.unwrap();
    let resumed = [0xf8, 0xff, 0xfe, 0x00];
    a.audio(7, seq.wrapping_add(3), &resumed).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let packet = b_audio.recv().await.unwrap();
            if packet.payload.as_ref() == resumed {
                assert_eq!(
                    packet.header.sequence_number, seq,
                    "intentional TS end marker must not count as RTP packet loss"
                );
                break;
            }
        }
    })
    .await
    .unwrap();
    a.close().await;
    b.close().await;
    drop(Arc::clone(&app));
}
