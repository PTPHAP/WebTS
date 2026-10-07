//! Opt-in live test: WEBTS_TEST_TARGET must point to an authorized, isolated TS3.
//! Uses protocol clients, not the official desktop client; no SMTP delivery claim.
use axum::http::HeaderValue;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tokio_util::sync::CancellationToken;
use tsclientlib::{Connection, Identity, MessageTarget, OutCommandExt, StreamItem};
use tsproto_packets::packets::{AudioData, CodecType, Flags, OutAudio};
use web_ts::{
    app::{App, router},
    config::{Config, Server},
    media::Media,
    password,
};

#[tokio::test]
#[ignore = "requires an explicitly authorized isolated TS3 server"]
async fn gateway_ts3_encrypted_voice_whisper_permission_and_revocation() {
    tokio::time::timeout(Duration::from_secs(90), run())
        .await
        .expect("live test timed out");
}

#[tokio::test]
#[ignore = "requires an explicitly authorized isolated TS3 server"]
async fn custom_target_alias_is_deduplicated_and_hot_policy_disconnects() {
    use tower::ServiceExt;
    let target = std::env::var("WEBTS_TEST_TARGET").expect("set isolated target");
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    std::fs::write(&key, hex::encode([18; 32])).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let origin = format!("http://localhost:{port}");
    let config: Config = toml::from_str(include_str!("../../config.example.toml")).unwrap();
    let app = App::new(Config {
        database: dir.path().join("db").to_string_lossy().into_owned(),
        master_key_file: key.to_string_lossy().into_owned(),
        public_url: origin.clone(),
        servers: vec![Server {
            id: "isolated".into(),
            name: "Test".into(),
            address: target.clone(),
        }],
        ..config
    })
    .unwrap();
    app.runtime.write().unwrap().settings.allow_custom = true;
    let hash = password::hash("custom fixture password").unwrap();
    let (owner, _, verify) = app.db.register("custom@example.com", &hash).unwrap();
    app.db.consume_email_token(&verify, "verify", None).unwrap();
    app.db.grant_admin("custom@example.com").unwrap();
    let session = app.db.create_session(owner, false, &hash).unwrap();
    let identity = Identity::create();
    let id = app
        .db
        .add_identity(owner, "custom", &identity, &app.vault)
        .unwrap();
    let server = tokio::spawn(
        axum::serve(
            listener,
            router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let mut request = format!("ws://127.0.0.1:{port}/api/connect")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_str(&origin).unwrap());
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("webts_dev={session}")).unwrap(),
    );
    let (mut first, _) = connect_async(request.clone()).await.unwrap();
    first.send(Message::Text(json!({"server":"","address":target,"identity":id,"page":"custom","name":"WebTS custom policy test"}).to_string().into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let Some(Ok(Message::Text(text))) = first.next().await else {
                panic!("unexpected close")
            };
            let message: Value = serde_json::from_str(&text).unwrap();
            assert_ne!(message["type"], "error", "{message}");
            if message["type"] == "state" {
                break;
            }
        }
    })
    .await
    .unwrap();
    let (mut duplicate, _) = connect_async(request).await.unwrap();
    duplicate
        .send(Message::Text(
            json!({"server":"isolated","identity":id,"page":"alias","name":"WebTS alias test"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Some(Ok(Message::Text(text))) = duplicate.next().await else {
                panic!("alias closed without refusal")
            };
            let message: Value = serde_json::from_str(&text).unwrap();
            if message["type"] == "error" {
                assert!(message["message"].as_str().unwrap().contains("已连接"));
                break;
            }
        }
    })
    .await
    .unwrap();
    let mut settings = app.runtime.read().unwrap().settings.clone();
    settings.allow_custom = false;
    let update = axum::http::Request::builder()
        .method("POST")
        .uri("/api/admin/settings")
        .header("origin", &origin)
        .header("cookie", format!("webts_dev={session}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            json!({"password":"custom fixture password","settings":settings}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        router(app.clone()).oneshot(update).await.unwrap().status(),
        axum::http::StatusCode::OK
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match first.next().await {
                Some(Ok(Message::Text(text))) => {
                    let message: Value = serde_json::from_str(&text).unwrap();
                    if message["type"] == "disconnected" {
                        break;
                    }
                }
                None | Some(Ok(Message::Close(_))) => break,
                _ => {}
            }
        }
    })
    .await
    .expect("custom connection survived policy change");
    assert!(!app.runtime.read().unwrap().settings.allow_custom);
    server.abort();
}

async fn run() {
    let target =
        std::env::var("WEBTS_TEST_TARGET").expect("set WEBTS_TEST_TARGET to an isolated TS3");
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    std::fs::write(&key, hex::encode([14; 32])).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let origin = format!("http://localhost:{port}");
    let config: Config = toml::from_str(include_str!("../../config.example.toml")).unwrap();
    let app = App::new(Config {
        database: dir.path().join("db").to_string_lossy().into_owned(),
        master_key_file: key.to_string_lossy().into_owned(),
        public_url: origin.clone(),
        servers: vec![Server {
            id: "isolated".into(),
            name: "Isolated test".into(),
            address: target.clone(),
        }],
        ..config
    })
    .unwrap();
    // This is an integration fixture, never an account-verification API bypass.
    let hash = password::hash("isolated fixture password").unwrap();
    let (owner, _, verify) = app.db.register("fixture@example.com", &hash).unwrap();
    app.db.consume_email_token(&verify, "verify", None).unwrap();
    let session = app.db.create_session(owner, false, &hash).unwrap();
    let identity = Identity::create();
    let identity_id = app
        .db
        .add_identity(owner, "live fixture", &identity, &app.vault)
        .unwrap();
    let server = tokio::spawn(
        axum::serve(
            listener,
            router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let mut native = Connection::build(target)
        .identity(Identity::create())
        .name("WebTS isolated protocol peer")
        .input_hardware_enabled(true)
        .output_hardware_enabled(true)
        .log_commands(false)
        .log_packets(false)
        .log_udp_packets(false)
        .connect()
        .unwrap();
    loop {
        if matches!(
            native.events().next().await.unwrap().unwrap(),
            StreamItem::BookEvents(_)
        ) {
            break;
        }
    }
    let native_id = native.get_state().unwrap().own_client.0;
    let mut request = format!("ws://127.0.0.1:{port}/api/connect")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_str(&origin).unwrap());
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("webts_dev={session}")).unwrap(),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();
    socket.send(Message::Text(json!({"server":"isolated","identity":identity_id,"page":"fixture","name":"WebTS isolated gateway"}).to_string().into())).await.unwrap();
    let (signals, mut signal_rx) = mpsc::channel(64);
    let (audio, mut audio_rx) = mpsc::channel(8);
    let cancel = CancellationToken::new();
    let mut peer = Media::new(&app, owner, 0, signals.clone(), audio, cancel.clone())
        .await
        .unwrap();
    peer.sync_speakers(&[7], &signals).await.unwrap();
    let mut timer = tokio::time::interval(Duration::from_millis(20));
    let payload = [0xf8, 0xff, 0xfe];
    let mut sequence = 0u16;
    let mut secure = false;
    let mut browser_voice = false;
    let mut ts_voice = false;
    let mut ts_whisper = false;
    let mut whisper_requested = false;
    let mut denied = false;
    let mut revoked = false;
    let mut own_id = 0u16;
    let mut browser_whisper = false;
    let mut chat_received = false;
    let mut browser_chat = false;
    let mut browser_poke = false;
    let whisper_payload = [0xf9, 0xff, 0xfe, 0xff, 0xfe];
    let deadline = tokio::time::sleep(Duration::from_secs(45));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _=&mut deadline=>panic!("incomplete live test: secure={secure}, normal(browser/ts)={browser_voice}/{ts_voice}, whisper(browser/ts)={browser_whisper}/{ts_whisper}, chat={chat_received}, denied={denied}, revoked={revoked}"),
            _=timer.tick()=>{
                if peer.check_cipher().await.unwrap().is_some() && secure {
                    peer.audio(7,sequence,&payload).await.unwrap();
                    let native_audio = if browser_voice && own_id != 0 {
                        AudioData::C2SWhisper{id:sequence,codec:CodecType::OpusVoice,clients:vec![own_id],channels:vec![],data:&whisper_payload}
                    } else { AudioData::C2S{id:sequence,codec:CodecType::OpusVoice,data:&payload} };
                    native.send_audio(OutAudio::new(&native_audio)).expect("protocol peer disconnected during voice test");
                    sequence=sequence.wrapping_add(1);
                }
            },
            Some(signal)=signal_rx.recv()=>if signal["type"]=="ice" {
                socket.send(Message::Text(signal.to_string().into())).await.unwrap();
            },
            Some(packet)=audio_rx.recv()=>{
                if packet.payload.as_ref()==whisper_payload {browser_whisper=true;} else {assert_eq!(packet.payload.as_ref(),payload);browser_voice=true;}
            },
            event=async{native.events().next().await}=>{
                let event = event.unwrap().unwrap();
                if let StreamItem::DisconnectedTemporarily(reason)=&event {panic!("protocol peer interrupted: {reason:?}");}
                if let StreamItem::BookEvents(events)=&event
                    && events.iter().any(|e|matches!(e,tsclientlib::events::Event::Message{message,..} if message=="WebTS live channel chat")) {
                    chat_received=true;
                }
                if let StreamItem::Audio(packet)=event{
                    assert!(!packet.data().packet().header().flags().contains(Flags::UNENCRYPTED));
                    match packet.data().data(){
                        AudioData::S2C{data,..}=>{assert_eq!(*data,payload);ts_voice=true;},
                        AudioData::S2CWhisper{data,..}=>{assert_eq!(*data,payload);ts_whisper=true;},
                        _=>{}
                    }
                }
            },
            event=socket.next()=>{
                let Some(Ok(Message::Text(text)))=event else {assert!(revoked,"unexpected WebSocket close");break;};
                let value:Value=serde_json::from_str(&text).unwrap();
                match value["type"].as_str().unwrap_or(""){
                    "offer"=>{
                        peer.peer.set_remote_description(serde_json::from_value(value["description"].clone()).unwrap()).await.unwrap();
                        let answer=peer.peer.create_answer(None).await.unwrap();
                        peer.peer.set_local_description(answer.clone()).await.unwrap();
                        socket.send(Message::Text(json!({"type":"answer","description":answer}).to_string().into())).await.unwrap();
                    },
                    "ice"=>peer.peer.add_ice_candidate(serde_json::from_value(value["candidate"].clone()).unwrap()).await.unwrap(),
                    "encryption"=>{assert_eq!(value["browser"],"SRTP_AEAD_AES_256_GCM");secure=true;
                        socket.send(Message::Text(json!({"type":"transmit","enabled":true}).to_string().into())).await.unwrap();
                        socket.send(Message::Text(json!({"type":"command","id":"denied","action":"kick","client":native_id,"scope":"server","text":"permission test"}).to_string().into())).await.unwrap();
                        socket.send(Message::Text(json!({"type":"command","id":"chat","action":"chat","scope":"channel","text":"WebTS live channel chat"}).to_string().into())).await.unwrap();
                    },
                    "state"=>{if own_id==0 {
                        own_id=value["own"].as_u64().unwrap() as u16;
                        native.get_state().unwrap().send_message(MessageTarget::Channel,"WebTS peer reply").send_with_result(&mut native).unwrap();
                        native.get_state().unwrap().send_message(MessageTarget::Poke(tsproto_types::ClientId(own_id)),"WebTS poke").send_with_result(&mut native).unwrap();
                    }},
                    "chat" if value["text"]=="WebTS peer reply"=>browser_chat=true,
                    "poke" if value["text"]=="WebTS poke"=>browser_poke=true,
                    "result" if value["id"]=="denied"=>{assert_eq!(value["ok"],false);assert!(value["message"].as_str().unwrap().to_lowercase().contains("permission"));denied=true;},
                    "error" if !revoked=>panic!("gateway rejected live test: {}",value["message"]),
                    "disconnected"=>{assert!(revoked);break;},
                    _=>{}
                }
            }
        }
        if ts_voice && browser_voice && !ts_whisper && !whisper_requested {
            socket
                .send(Message::Text(
                    json!({"type":"whisper","clients":[native_id],"channels":[]})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            whisper_requested = true;
        }
        if ts_whisper
            && browser_voice
            && browser_whisper
            && chat_received
            && browser_chat
            && browser_poke
            && denied
            && !revoked
        {
            app.db.revoke(owner, None).unwrap();
            app.connections.cancel(owner, None, None);
            revoked = true;
        }
    }
    assert!(
        secure
            && browser_voice
            && browser_whisper
            && ts_voice
            && ts_whisper
            && chat_received
            && denied
            && revoked
    );
    peer.close().await;
    server.abort();
    println!(
        "PASS: live TS3 / WebRTC AES-256-GCM bidirectional Opus and encrypted whispers, channel chat, permission refusal, session revocation"
    );
    if std::env::var("WEBTS_TEST_KEEP_FIXTURE").as_deref() == Ok("1") {
        println!(
            "Local UI fixture (verified by test setup, no SMTP claim): {}",
            dir.keep().display()
        );
    }
}
