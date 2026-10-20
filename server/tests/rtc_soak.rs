//! Opt-in loopback capacity evidence, not Linux/TS/client audio acceptance.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;
use web_ts::{
    app::App,
    config::Config,
    media::{InboundAudio, Media},
};
async fn negotiate(
    sender: &mut Media,
    receiver: &mut Media,
    events: &mut mpsc::Receiver<Value>,
    signals: &mpsc::Sender<Value>,
) -> Result<()> {
    sender.offer(signals).await?;
    let mut candidates = Vec::new();
    let offer = loop {
        let e = events.recv().await.context("signal ended")?;
        if e["type"] == "offer" {
            break serde_json::from_value(e["description"].clone())?;
        }
        if e["type"] == "ice" {
            candidates.push(e["candidate"].clone());
        }
    };
    receiver.peer.set_remote_description(offer).await?;
    for c in candidates {
        receiver
            .peer
            .add_ice_candidate(serde_json::from_value(c)?)
            .await?;
    }
    let answer = receiver.peer.create_answer(None).await?;
    receiver.peer.set_local_description(answer.clone()).await?;
    sender.answer(answer).await?;
    Ok(())
}
#[derive(Default)]
struct Totals {
    received: u64,
    sent: u64,
    max_gap_ms: u64,
    latency_us: u128,
    latency_max_us: u128,
    matched: u64,
    latency_buckets: Vec<u64>,
    streams: usize,
}
async fn pair(
    app: Arc<App>,
    index: usize,
    active: bool,
    speakers: usize,
    slots: usize,
    stop: CancellationToken,
    ready: mpsc::Sender<usize>,
) -> Result<Totals> {
    let (at, mut ar) = mpsc::channel(64);
    let (bt, mut br) = mpsc::channel(64);
    let (aa, mut a_audio) = mpsc::channel::<InboundAudio>((speakers * 2).max(8));
    let (ba, mut b_audio) = mpsc::channel::<InboundAudio>((speakers * 2).max(8));
    let mut a = Media::new(&app, 1, 0, at.clone(), aa, stop.child_token()).await?;
    let mut b = Media::new(&app, 2, 0, bt.clone(), ba, stop.child_token()).await?;
    let outcome=async{
  let ids=if speakers>0{(1..=slots as u16).collect::<Vec<_>>()}else{vec![7]};
  a.sync_speakers(&ids,&at).await?;b.sync_speakers(if speakers>0{&[]}else{&[8]},&bt).await?;
  tokio::time::timeout(Duration::from_secs(20),async{negotiate(&mut a,&mut b,&mut ar,&at).await?;negotiate(&mut b,&mut a,&mut br,&bt).await?;anyhow::Ok(())}).await??;
  let mut tick=tokio::time::interval(Duration::from_millis(if active{20}else{500}));tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
  let mut started=false;let mut totals=Totals::default();let mut seq=0u16;let deadline=Instant::now();let mut cipher_check=Instant::now()-Duration::from_secs(1);let mut last_a=HashMap::new();let mut last_b=HashMap::new();let mut sent=VecDeque::new();
  loop{tokio::select!{
   _=stop.cancelled()=>break,
   Some(e)=ar.recv()=>if e["type"]=="ice"{b.peer.add_ice_candidate(serde_json::from_value(e["candidate"].clone())?).await?;},
   Some(e)=br.recv()=>if e["type"]=="ice"{a.peer.add_ice_candidate(serde_json::from_value(e["candidate"].clone())?).await?;},
   _=tick.tick()=>{
    if cipher_check.elapsed()>=Duration::from_secs(1){
     let ac=a.check_cipher().await?;let bc=b.check_cipher().await?;cipher_check=Instant::now();
     if let(Some(ac),Some(bc))=(ac,bc){if ac["browser"]!="SRTP_AEAD_AES_256_GCM"||bc["browser"]!="SRTP_AEAD_AES_256_GCM"{bail!("cipher floor failed");}
      if !started{ready.send(index).await?;started=true;}
     }else if deadline.elapsed()>Duration::from_secs(20)&&!started{bail!("cipher negotiation timeout");}
    }
    if started&&active{let now=Instant::now();if speakers>0{for id in 1..=speakers as u16{a.audio(id,seq,&[0xf8,0xff,0xfe]).await?;}}else{a.audio(7,seq,&[0xf8,0xff,0xfe]).await?;b.audio(8,seq,&[0xf8,0xff,0xfe]).await?;}sent.push_back((seq,now));while sent.len()>128{sent.pop_front();}seq=seq.wrapping_add(1);totals.sent+=if speakers>0{speakers as u64}else{2};}
   },
   packet=a_audio.recv()=>record(packet,&mut totals,&mut last_a,&sent)?,
   packet=b_audio.recv()=>record(packet,&mut totals,&mut last_b,&sent)?,
  }}
  totals.streams=last_a.len()+last_b.len();
  if active{let expected=if speakers>0{speakers}else{2};if totals.sent==0||totals.streams!=expected||last_a.values().chain(last_b.values()).any(|(_,n)|*n as f64/(totals.sent as f64/expected as f64)<=0.99){bail!("pair {index}: a stream is missing or encrypted audio delivery below 99%; expected {expected} streams, sent {}, received {:?}/{:?}, max gap {}ms",totals.sent,last_a.iter().map(|(ssrc,(_,n))|(*ssrc,*n)).collect::<Vec<_>>(),last_b.iter().map(|(ssrc,(_,n))|(*ssrc,*n)).collect::<Vec<_>>(),totals.max_gap_ms);}}Ok(totals)
 }.await;
    a.close().await;
    b.close().await;
    outcome
}
fn record(
    packet: Option<InboundAudio>,
    totals: &mut Totals,
    last: &mut HashMap<u32, (Instant, u64)>,
    sent: &VecDeque<(u16, Instant)>,
) -> Result<()> {
    let packet = packet.context("audio channel ended")?;
    if packet.packet.payload.as_ref() != [0xf8, 0xff, 0xfe] {
        bail!("audio mismatch");
    }
    let now = Instant::now();
    totals.received += 1;
    if let Some((old, count)) = last.get_mut(&packet.packet.header.ssrc) {
        totals.max_gap_ms = totals
            .max_gap_ms
            .max(now.duration_since(*old).as_millis() as u64);
        *old = now;
        *count += 1;
    } else {
        last.insert(packet.packet.header.ssrc, (now, 1));
    }
    if let Some((_, at)) = sent
        .iter()
        .rev()
        .find(|(seq, _)| *seq == packet.packet.header.sequence_number)
    {
        let us = now.duration_since(*at).as_micros();
        totals.latency_us += us;
        totals.latency_max_us = totals.latency_max_us.max(us);
        totals.matched += 1;
        if totals.latency_buckets.is_empty() {
            totals.latency_buckets.resize(1001, 0);
        }
        totals.latency_buckets[us.div_ceil(1000).min(1000) as usize] += 1;
    }
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "isolated loopback, 50 connections / 10 synthetic Opus streams, 30 minutes"]
async fn fifty_connections_ten_voice_loopback_soak() {
    let pairs = std::env::var("WEBTS_SOAK_PAIRS")
        .map(|s| s.parse::<usize>().expect("invalid WEBTS_SOAK_PAIRS"))
        .unwrap_or(50);
    let active = std::env::var("WEBTS_SOAK_ACTIVE_PAIRS")
        .map(|s| s.parse::<usize>().expect("invalid WEBTS_SOAK_ACTIVE_PAIRS"))
        .unwrap_or(10);
    assert!((1..=200).contains(&pairs) && (1..=pairs).contains(&active));
    let speakers = std::env::var("WEBTS_SOAK_SPEAKERS")
        .map(|s| s.parse::<usize>().expect("invalid WEBTS_SOAK_SPEAKERS"))
        .unwrap_or(0);
    assert!(speakers == 0 || (speakers <= pairs && pairs <= 128 && active == pairs));
    let seconds = std::env::var("WEBTS_SOAK_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1800);
    assert!((10..=7200).contains(&seconds));
    let root = tempfile::tempdir().unwrap();
    let key = root.path().join("key");
    std::fs::write(&key, hex::encode([9; 32])).unwrap();
    let template: Config = toml::from_str(include_str!("../../config.example.toml")).unwrap();
    let app = App::new(Config {
        database: root.path().join("db").to_string_lossy().into_owned(),
        master_key_file: key.to_string_lossy().into_owned(),
        ..template
    })
    .unwrap();
    let stop = CancellationToken::new();
    let mut jobs = JoinSet::new();
    let (tx, mut rx) = mpsc::channel(64);
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(if speakers > 0 {
            ".cache/voice-fanout-20261020"
        } else {
            ".cache/voice-community-20261015"
        });
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join("soak-running.json"),
        json!({"pid":std::process::id(),"seconds":seconds,"state":"starting"}).to_string(),
    )
    .unwrap();
    for index in 0..pairs {
        jobs.spawn(pair(
            app.clone(),
            index,
            index < active,
            speakers,
            pairs,
            stop.child_token(),
            tx.clone(),
        ));
    }
    let ready=tokio::time::timeout(Duration::from_secs(90),async{for _ in 0..pairs{tokio::select!{v=rx.recv()=>{v.expect("startup channel");},r=jobs.join_next()=>panic!("pair ended early: {}",r.unwrap().unwrap().err().map(|e|e.to_string()).unwrap_or_default())}}}).await;
    if ready.is_err() {
        stop.cancel();
        while let Some(r) = jobs.join_next().await {
            let _ = r;
        }
        panic!("soak startup timed out");
    }
    std::fs::write(output.join("soak-running.json"),json!({"pid":std::process::id(),"seconds":seconds,"state":"running","connections":pairs,"active":active}).to_string()).unwrap();
    let start = Instant::now();
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    stop.cancel();
    let mut totals = Totals::default();
    totals.latency_buckets.resize(1001, 0);
    while let Some(r) = jobs.join_next().await {
        let v = r.unwrap().unwrap();
        totals.received += v.received;
        totals.sent += v.sent;
        totals.max_gap_ms = totals.max_gap_ms.max(v.max_gap_ms);
        totals.latency_us += v.latency_us;
        totals.latency_max_us = totals.latency_max_us.max(v.latency_max_us);
        totals.matched += v.matched;
        totals.streams += v.streams;
        for (i, n) in v.latency_buckets.into_iter().enumerate() {
            totals.latency_buckets[i] += n;
        }
    }
    let rank = totals.matched.saturating_mul(95).div_ceil(100);
    let mut cumulative = 0;
    let p95 = totals
        .latency_buckets
        .iter()
        .enumerate()
        .find_map(|(i, n)| {
            cumulative += n;
            (cumulative >= rank && rank > 0 && (i < 1000 || totals.latency_max_us <= 1_000_000))
                .then_some(i)
        });
    let report = json!({"scope":"Windows loopback RTC only; synthetic Opus silence; not TS, physical microphone or reference Linux acceptance","seconds":start.elapsed().as_secs_f64(),"connections":pairs,"active_bidirectional_pairs":active,"cipher":"SRTP_AEAD_AES_256_GCM","sent":totals.sent,"received":totals.received,"max_receive_gap_ms":totals.max_gap_ms,"matched_timing_samples":totals.matched,"mean_loopback_transit_ms":totals.latency_us as f64/totals.matched as f64/1000.0,"p95_loopback_transit_upper_ms":p95,"max_loopback_transit_ms":totals.latency_max_us as f64/1000.0,"closed_peers":pairs*2});
    let mut report = report;
    report["mode"] = json!(if speakers > 0 {
        "speaker-fanout"
    } else {
        "bidirectional-pairs"
    });
    report["speaker_streams"] = json!(totals.streams);
    if speakers > 0 {
        report["active_bidirectional_pairs"] = json!(0);
        report["speakers_per_listener"] = json!(speakers);
        report["preallocated_speaker_slots_per_listener"] = json!(pairs);
    }
    std::fs::write(
        output.join(if seconds >= 1800 {
            "soak-30min.json"
        } else {
            "soak-smoke.json"
        }),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{}", report);
    assert!(
        totals.received as f64 / totals.sent as f64 > 0.99,
        "loopback delivery below 99%"
    );
}
