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
    let offer = a.peer.create_offer(None).await.unwrap();
    a.peer.set_local_description(offer.clone()).await.unwrap();
    b.peer.set_remote_description(offer).await.unwrap();
    let answer = b.peer.create_answer(None).await.unwrap();
    b.peer.set_local_description(answer.clone()).await.unwrap();
    a.answer(answer).await.unwrap();
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
    a.close().await;
    b.close().await;
    drop(Arc::clone(&app));
}
