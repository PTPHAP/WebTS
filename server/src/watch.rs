//! A single administrator-curated watch event. Video is never relayed by WebTS.
use crate::{
    app::{Api, App, Error},
    db::{Db, Session, now},
    settings,
    vault::Vault,
};
use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use hmac::{Hmac, Mac};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
};
use zeroize::Zeroizing;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub title: String,
    pub description: String,
    pub url: String,
    pub cover: String,
    pub public: bool,
    pub embed_enabled: bool,
    pub ts_server: String,
    pub ts_channel: u32,
    pub score_mode: ScoreMode,
}
#[derive(Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreMode {
    #[default]
    None,
    Manual,
    Webhook,
}
#[derive(Default, Deserialize, Serialize)]
struct Stored {
    config: Config,
    revision: u64,
    event_id: String,
    feed_key: String,
}
struct Live {
    stored: Stored,
    score: Option<Snapshot>,
    sequence: u64,
}
pub struct Watch {
    live: RwLock<Live>,
    inspections: tokio::sync::Semaphore,
}
impl Watch {
    pub fn load(db: &Db, vault: &Vault) -> anyhow::Result<Self> {
        let ciphertext: Option<Vec<u8>> = db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT ciphertext FROM watch_settings WHERE id=1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let stored: Stored = if let Some(bytes) = ciphertext {
            serde_json::from_slice(&vault.open(0, "watch-settings", "v1", &bytes)?)?
        } else {
            Stored::default()
        };
        let sequence = db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT sequence FROM watch_feed_state WHERE id=1 AND event_id=?",
                [&stored.event_id],
                |r| r.get::<_, u64>(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(Self {
            live: RwLock::new(Live {
                stored,
                score: None,
                sequence,
            }),
            inspections: tokio::sync::Semaphore::new(2),
        })
    }
}
fn text(s: &str, max: usize, multiline: bool) -> bool {
    s.chars().count() <= max
        && !s
            .chars()
            .any(|c| c.is_control() && !(multiline && c == '\n'))
}
pub fn source(value: &str) -> Api<(String, &'static str)> {
    let mut u = url::Url::parse(value).map_err(|_| Error::bad("请输入完整 HTTPS 直播间链接"))?;
    if value.len() > 2048
        || value.chars().any(char::is_control)
        || u.scheme() != "https"
        || u.port_or_known_default() != Some(443)
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err(Error::bad("直播链接仅支持无账号凭据的公网 HTTPS 默认端口"));
    }
    let host = u.host().ok_or_else(|| Error::bad("直播链接缺少主机"))?;
    match host {
        url::Host::Ipv4(ip) if !tsclientlib::resolver::is_public_addr(&ip.into()) => {
            return Err(Error::bad("不允许内网地址"));
        }
        url::Host::Ipv6(ip) if !tsclientlib::resolver::is_public_addr(&ip.into()) => {
            return Err(Error::bad("不允许内网地址"));
        }
        url::Host::Domain(h)
            if !h.contains('.')
                || h.ends_with(".local")
                || h.ends_with(".localhost")
                || h == "localhost" =>
        {
            return Err(Error::bad("不允许内网主机"));
        }
        _ => {}
    }
    u.set_fragment(None);
    let h = u.host_str().unwrap().to_ascii_lowercase();
    let path = u.path().trim_matches('/').to_owned();
    let numeric = |v: &str| !v.is_empty() && v.len() <= 18 && v.bytes().all(|b| b.is_ascii_digit());
    let platform = match h.as_str() {
        "live.bilibili.com" => {
            if !numeric(&path) {
                return Err(Error::bad("B站链接应为 live.bilibili.com/房间号"));
            }
            u.set_query(None);
            "bilibili"
        }
        "huya.com" | "www.huya.com" => {
            if path.is_empty()
                || path.len() > 64
                || !path.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(Error::bad("虎牙链接应指向一个直播间"));
            }
            u.set_host(Some("www.huya.com")).unwrap();
            u.set_query(None);
            "huya"
        }
        "douyu.com" | "www.douyu.com" => {
            if !numeric(&path) {
                return Err(Error::bad("斗鱼链接应指向数字直播间"));
            }
            u.set_host(Some("www.douyu.com")).unwrap();
            u.set_query(None);
            "douyu"
        }
        "youtube.com" | "www.youtube.com" | "youtu.be" => {
            let id = if h == "youtu.be" {
                Some(path.clone())
            } else if path == "watch" {
                u.query_pairs()
                    .find(|(k, _)| k == "v")
                    .map(|(_, v)| v.into_owned())
            } else {
                path.strip_prefix("live/").map(str::to_owned)
            };
            let id = id
                .filter(|s| {
                    s.len() == 11
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                })
                .ok_or_else(|| Error::bad("请输入 YouTube 视频或直播链接"))?;
            u = url::Url::parse(&format!("https://www.youtube.com/watch?v={id}")).unwrap();
            "youtube"
        }
        "twitch.tv" | "www.twitch.tv" => {
            if path.is_empty()
                || path.len() > 25
                || !path.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(Error::bad("请输入 Twitch 频道链接"));
            }
            u.set_host(Some("www.twitch.tv")).unwrap();
            u.set_query(None);
            "twitch"
        }
        _ => {
            if [".mp4", ".m3u8", ".mpd", ".flv", ".ts"]
                .iter()
                .any(|ext| path.to_ascii_lowercase().ends_with(ext))
            {
                return Err(Error::bad("请输入直播间网页，本站不代理视频流"));
            }
            "link"
        }
    };
    Ok((u.to_string(), platform))
}
fn view(live: &Live, admin: bool) -> Value {
    let s = &live.stored;
    if !admin && !s.config.enabled {
        return json!({"enabled":false});
    }
    json!({"enabled":s.config.enabled,"config":s.config,"revision":s.revision,"event_id":s.event_id,
        "platform":source(&s.config.url).map(|v|v.1).unwrap_or("link"),"score":live.score,
        "server_time":now(),"feed_key_set":admin && !s.feed_key.is_empty()})
}
pub async fn get(State(a): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    a.work(move |a| {
        let live = a.watch.live.read().unwrap();
        if live.stored.config.enabled && !live.stored.config.public {
            a.session(&headers)?;
        }
        Ok(Json(view(&live, false)))
    })
    .await
}
pub async fn admin_get(State(a): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    a.work(move |a| {
        settings::admin(a, &headers)?;
        Ok(Json(view(&a.watch.live.read().unwrap(), true)))
    })
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    password: String,
    revision: u64,
    config: Config,
}
fn persist(a: &App, session: &Session, stored: &Stored) -> Api<()> {
    let raw = Zeroizing::new(serde_json::to_vec(stored).map_err(anyhow::Error::from)?);
    let encrypted = a.vault.seal(0, "watch-settings", "v1", &raw)?;
    let changed=a.db.connection.lock().unwrap().execute("INSERT INTO watch_settings(id,ciphertext) SELECT 1,? WHERE EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.hash=? AND u.id=? AND s.expires>? AND u.verified=1 AND u.is_admin=1 AND u.banned_until!=-1 AND u.banned_until<=?) ON CONFLICT(id) DO UPDATE SET ciphertext=excluded.ciphertext",params![encrypted,session.hash,session.user.id,now(),now()]).map_err(anyhow::Error::from)?;
    if changed != 1 {
        return Err(Error(StatusCode::UNAUTHORIZED, "管理员会话已失效"));
    }
    Ok(())
}
pub async fn save(
    State(a): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Update>,
) -> Api<Json<Value>> {
    a.expensive(move |a| {
        settings::admin(a, &headers)?;
        let session = a.reauthenticate(&headers, &Zeroizing::new(body.password))?;
        let c = &mut body.config;
        if !text(&c.title, 120, false)
            || !text(&c.description, 1200, true)
            || !text(&c.ts_server, 64, false)
            || (c.enabled && c.title.trim().is_empty())
        {
            return Err(Error::bad("标题、介绍或服务器编号无效"));
        }
        if !c.url.is_empty() {
            c.url = source(&c.url)?.0;
        } else if c.enabled {
            return Err(Error::bad("启用前请填写直播间链接"));
        }
        crate::site::image(&mut c.cover)?;
        if !c.ts_server.is_empty()
            && !a
                .runtime
                .read()
                .unwrap()
                .settings
                .servers
                .iter()
                .any(|s| s.id == c.ts_server)
        {
            return Err(Error::bad("语音服务器不在站长配置列表中"));
        }
        let mut live = a.watch.live.write().unwrap();
        if body.revision != live.stored.revision {
            return Err(Error(StatusCode::CONFLICT, "配置已被更新，请重新加载"));
        }
        let reset = c.url != live.stored.config.url
            || c.enabled != live.stored.config.enabled
            || c.score_mode != live.stored.config.score_mode;
        let stored = Stored {
            config: body.config,
            revision: body
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::bad("版本超限"))?,
            event_id: if reset || live.stored.event_id.is_empty() {
                crate::vault::token()?
            } else {
                live.stored.event_id.clone()
            },
            feed_key: live.stored.feed_key.clone(),
        };
        persist(a, &session, &stored)?;
        live.stored = stored;
        if reset {
            live.score = None;
            live.sequence = 0;
        }
        Ok(Json(view(&live, true)))
    })
    .await
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub score: u16,
    pub maps_won: u8,
    pub side: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Player {
    pub name: String,
    pub team: String,
    pub kills: u16,
    pub deaths: u16,
    pub assists: u16,
    pub adr: Option<f32>,
    pub health: Option<u8>,
    pub armor: Option<u8>,
    pub money: Option<u16>,
    pub weapon: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    pub name: String,
    pub scores: [u16; 2],
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Score {
    pub tournament: String,
    pub stage: String,
    pub status: String,
    pub best_of: u8,
    pub teams: [Team; 2],
    pub map: String,
    pub round: u16,
    pub phase: String,
    pub bomb: Option<String>,
    pub players: Vec<Player>,
    pub maps: Vec<Map>,
    pub rounds: Vec<String>,
    pub events: Vec<String>,
}
impl Score {
    fn validate(&self) -> Api<()> {
        let mut ids = HashSet::new();
        let mut valid = text(&self.tournament, 120, false)
            && text(&self.stage, 120, false)
            && matches!(self.status.as_str(), "scheduled" | "live" | "finished")
            && [1, 3, 5, 7].contains(&self.best_of)
            && text(&self.map, 64, false)
            && self.round <= 100
            && text(&self.phase, 64, false)
            && self.bomb.as_ref().is_none_or(|s| text(s, 64, false))
            && self.players.len() <= 24
            && self.maps.len() <= 7
            && self.rounds.len() <= 100
            && self.events.len() <= 50;
        for t in &self.teams {
            valid &= !t.id.is_empty()
                && text(&t.id, 32, false)
                && ids.insert(t.id.as_str())
                && !t.name.trim().is_empty()
                && text(&t.name, 80, false)
                && t.score <= 100
                && t.maps_won <= 7
                && t.side.as_ref().is_none_or(|s| s == "T" || s == "CT");
        }
        for p in &self.players {
            valid &= text(&p.name, 64, false)
                && ids.contains(p.team.as_str())
                && p.kills <= 1000
                && p.deaths <= 1000
                && p.assists <= 1000
                && p.adr
                    .is_none_or(|n| n.is_finite() && (0.0..=1000.0).contains(&n))
                && p.health.is_none_or(|n| n <= 100)
                && p.armor.is_none_or(|n| n <= 100)
                && p.money.is_none_or(|n| n <= 65500)
                && p.weapon.as_ref().is_none_or(|s| text(s, 64, false));
        }
        valid &= self
            .maps
            .iter()
            .all(|m| text(&m.name, 64, false) && m.scores.iter().all(|n| *n <= 100))
            && self.rounds.iter().all(|id| ids.contains(id.as_str()))
            && self.events.iter().all(|s| text(s, 160, false));
        if !valid {
            return Err(Error::bad("CS2 赛况字段无效或超过数量限制"));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
struct Snapshot {
    data: Score,
    updated_at: i64,
    source: &'static str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manual {
    event_id: String,
    score: Score,
}
pub async fn manual(
    State(a): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Manual>,
) -> Api<Json<Value>> {
    a.work(move |a| {
        body.score.validate()?;
        let mut live = a.watch.live.write().unwrap();
        settings::admin(a, &headers)?;
        if !live.stored.config.enabled
            || live.stored.config.score_mode != ScoreMode::Manual
            || body.event_id != live.stored.event_id
        {
            return Err(Error(
                StatusCode::CONFLICT,
                "当前活动不接受手动赛况，请重新加载",
            ));
        }
        live.score = Some(Snapshot {
            data: body.score,
            updated_at: now(),
            source: "manual",
        });
        Ok(Json(json!({"message":"手动赛况已更新"})))
    })
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rotate {
    password: String,
    revision: u64,
}
pub async fn rotate(
    State(a): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Rotate>,
) -> Api<Json<Value>> {
    a.expensive(move |a| {
        settings::admin(a, &headers)?;
        let s = a.reauthenticate(&headers, &Zeroizing::new(body.password))?;
        let mut live = a.watch.live.write().unwrap();
        if body.revision != live.stored.revision {
            return Err(Error(StatusCode::CONFLICT, "配置版本已变化，请重新加载"));
        }
        let key = crate::vault::token()?;
        let stored = Stored {
            config: live.stored.config.clone(),
            revision: body.revision + 1,
            event_id: crate::vault::token()?,
            feed_key: key.clone(),
        };
        persist(a, &s, &stored)?;
        live.stored = stored;
        live.score = None;
        live.sequence = 0;
        Ok(Json(
            json!({"key":key,"revision":live.stored.revision,"event_id":live.stored.event_id}),
        ))
    })
    .await
}
fn header<'a>(h: &'a HeaderMap, k: &str) -> Api<&'a str> {
    h.get(k)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error(StatusCode::UNAUTHORIZED, "赛况签名无效"))
}
fn authenticate(h: &HeaderMap, raw: &[u8], stored: &Stored, last: u64) -> Api<u64> {
    let timestamp = header(h, "x-webts-timestamp")?;
    let sequence = header(h, "x-webts-sequence")?;
    let event = header(h, "x-webts-event")?;
    let invalid = || Error(StatusCode::UNAUTHORIZED, "赛况签名无效");
    let time = timestamp.parse::<i64>().map_err(|_| invalid())?;
    let seq = sequence.parse::<u64>().map_err(|_| invalid())?;
    if stored.feed_key.is_empty() || now().abs_diff(time) > 60 || event != stored.event_id {
        return Err(invalid());
    }
    let mut mac =
        Hmac::<Sha256>::new_from_slice(stored.feed_key.as_bytes()).map_err(|_| invalid())?;
    mac.update(format!("{timestamp}\n{sequence}\n{event}\n").as_bytes());
    mac.update(raw);
    mac.verify_slice(&hex::decode(header(h, "x-webts-signature")?).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    if seq <= last || seq > i64::MAX as u64 {
        return Err(Error(StatusCode::CONFLICT, "赛况序号重复或过期"));
    }
    Ok(seq)
}
pub async fn feed(State(a): State<Arc<App>>, headers: HeaderMap, raw: Bytes) -> Api<Json<Value>> {
    if raw.len() > 64 * 1024 {
        return Err(Error::bad("赛况请求过大"));
    }
    a.work(move |a| {let mut live=a.watch.live.write().unwrap();let sequence=authenticate(&headers,&raw,&live.stored,live.sequence)?;
        if !live.stored.config.enabled || live.stored.config.score_mode!=ScoreMode::Webhook {return Err(Error(StatusCode::CONFLICT,"赛况接口尚未启用"));}
        let score:Score=serde_json::from_slice(&raw).map_err(|_|Error::bad("赛况 JSON 无效"))?;score.validate()?;
        // Store only the anti-replay counter, never a match archive. Restarts must not accept old signatures.
        let changed=a.db.connection.lock().unwrap().execute("INSERT INTO watch_feed_state(id,event_id,sequence) VALUES(1,?,?) ON CONFLICT(id) DO UPDATE SET event_id=excluded.event_id,sequence=excluded.sequence WHERE watch_feed_state.event_id!=excluded.event_id OR watch_feed_state.sequence<excluded.sequence",params![live.stored.event_id,sequence as i64]).map_err(anyhow::Error::from)?;
        if changed!=1 {return Err(Error(StatusCode::CONFLICT,"赛况序号重复或过期"));}
        live.sequence=sequence;live.score=Some(Snapshot{data:score,updated_at:now(),source:"webhook"});Ok(Json(json!({"accepted":sequence})))
    }).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    url: String,
}
pub async fn inspect(
    State(a): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Inspect>,
) -> Api<Json<Value>> {
    let h = headers.clone();
    a.work(move |a| settings::admin(a, &h).map(|_| ())).await?;
    let _permit = a.watch.inspections.try_acquire().map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "已有直播页读取任务，请稍后再试",
        )
    })?;
    let (url, platform) = source(&body.url)?;
    let raw = crate::server_banner::download_page(&url)
        .await
        .map_err(|_| Error::bad("无法读取公开网页，请手动填写标题和封面；不绕过平台限制"))?;
    let h = headers.clone();
    let meta = a
        .work(move |a| {
            settings::admin(a, &h)?;
            Ok(crate::watch_metadata::parse(&String::from_utf8_lossy(&raw)))
        })
        .await?;
    let image_url = url::Url::parse(&url)
        .ok()
        .and_then(|u| u.join(&meta.2).ok())
        .filter(|u| !meta.2.is_empty() && u.scheme() == "https");
    let bytes = if let Some(u) = image_url {
        crate::server_banner::download(u.as_str(), 1024 * 1024)
            .await
            .ok()
    } else {
        None
    };
    a.expensive(move|a|{settings::admin(a,&headers)?;let cover=bytes.and_then(|b|cover(&b).ok()).unwrap_or_default();Ok(Json(json!({"url":url,"platform":platform,"title":meta.0,"description":meta.1,"cover":cover,"note":"仅读取公开网页元信息；读取不到的字段请手动补充。实时比赛数据需单独来源。"})))}).await
}
fn cover(bytes: &[u8]) -> Api<String> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use image::{ImageReader, Limits};
    use std::io::Cursor;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::bad("封面无效"))?;
    if !matches!(
        reader.format(),
        Some(
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::WebP
                | image::ImageFormat::Gif
        )
    ) {
        return Err(Error::bad("封面格式不支持"));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| Error::bad("封面无效或过大"))?
        .thumbnail(1280, 960);
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 75)
        .encode_image(&image)
        .map_err(|_| Error::bad("封面处理失败"))?;
    if out.len() > 128 * 1024 {
        return Err(Error::bad("封面过大，请手动上传"));
    }
    Ok(format!("data:image/jpeg;base64,{}", STANDARD.encode(out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn direct_watch_navigation_serves_the_shell_with_success_status() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let (dir, mut a, _) = crate::app::tests::fixture();
        let web = dir.path().join("watch-web");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(
            web.join("index.html"),
            "<!doctype html><title>Watch shell</title>",
        )
        .unwrap();
        Arc::get_mut(&mut a).unwrap().config.web_dir = web.to_str().unwrap().into();
        let response = crate::app::router(a)
            .oneshot(
                Request::builder()
                    .uri("/watch")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .contains("text/html")
        );
    }
    #[test]
    fn sources_are_canonical_and_never_accept_private_or_video_urls() {
        assert_eq!(
            source("https://live.bilibili.com/123?from=share#x").unwrap(),
            ("https://live.bilibili.com/123".into(), "bilibili")
        );
        assert_eq!(
            source("https://youtu.be/abcdefghijk?t=30").unwrap().0,
            "https://www.youtube.com/watch?v=abcdefghijk"
        );
        for s in [
            "http://www.huya.com/test",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://169.254.169.254/",
            "https://a.local/x",
            "https://u:p@www.huya.com/test",
            "https://live.bilibili.com.evil.invalid/x.mp4",
            "https://live.bilibili.com/123/other",
            "https://www.huya.com:8443/test",
        ] {
            assert!(source(s).is_err(), "{s}");
        }
        assert_eq!(
            source("https://live.bilibili.com.evil.invalid/123")
                .unwrap()
                .1,
            "link"
        );
    }
    #[test]
    fn signature_covers_event_time_sequence_and_exact_body_and_rejects_replay() {
        let stored = Stored {
            feed_key: "a-secret-key".into(),
            event_id: "event".into(),
            ..Default::default()
        };
        let body = b"{}";
        let time = now().to_string();
        let prefix = format!("{time}\n1\nevent\n");
        let mut mac = Hmac::<Sha256>::new_from_slice(stored.feed_key.as_bytes()).unwrap();
        mac.update(prefix.as_bytes());
        mac.update(body);
        let mut h = HeaderMap::new();
        for (k, v) in [
            ("x-webts-timestamp", time),
            ("x-webts-sequence", "1".into()),
            ("x-webts-event", "event".into()),
            (
                "x-webts-signature",
                hex::encode(mac.finalize().into_bytes()),
            ),
        ] {
            h.insert(
                axum::http::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        assert_eq!(authenticate(&h, body, &stored, 0).unwrap(), 1);
        assert!(authenticate(&h, b"{ }", &stored, 0).is_err());
        assert_eq!(
            authenticate(&h, body, &stored, 1).unwrap_err().0,
            StatusCode::CONFLICT
        );
        h.insert("x-webts-event", "other".parse().unwrap());
        assert!(authenticate(&h, body, &stored, 0).is_err());
    }
    #[test]
    fn defaults_do_not_leak_inactive_config_or_key() {
        let live = Live {
            stored: Stored {
                feed_key: "private".into(),
                ..Default::default()
            },
            score: None,
            sequence: 0,
        };
        assert_eq!(view(&live, false), json!({"enabled":false}));
        assert!(!view(&live, true).to_string().contains("private"));
    }
    #[tokio::test]
    async fn watch_visibility_admin_and_machine_feed_have_separate_authority() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let (_dir, a, headers) = crate::app::tests::fixture();
        {
            let mut l = a.watch.live.write().unwrap();
            l.stored.config.enabled = true;
            l.stored.config.url = "https://live.bilibili.com/123".into();
            l.stored.config.title = "Members only".into();
        }
        let router = crate::app::router(a.clone());
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/watch")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let mut request = Request::builder()
            .uri("/api/watch")
            .body(Body::empty())
            .unwrap();
        *request.headers_mut() = headers.clone();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::OK
        );
        let mut request = Request::builder()
            .uri("/api/admin/watch")
            .body(Body::empty())
            .unwrap();
        *request.headers_mut() = headers;
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let request = Request::builder()
            .method("POST")
            .uri("/api/watch/feed")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let request = Request::builder()
            .method("POST")
            .uri("/api/admin/watch")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            router.oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn write_rechecks_admin_ban_and_session_in_database() {
        let (_dir, a, headers) = crate::app::tests::fixture();
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=1 WHERE id=1", [])
            .unwrap();
        let session = a.session(&headers).unwrap();
        let stored = Stored {
            feed_key: "not-plaintext-in-db".into(),
            ..Default::default()
        };
        persist(&a, &session, &stored).unwrap();
        let raw: Vec<u8> =
            a.db.connection
                .lock()
                .unwrap()
                .query_row("SELECT ciphertext FROM watch_settings", [], |r| r.get(0))
                .unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("not-plaintext-in-db"));
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=-1 WHERE id=1", [])
            .unwrap();
        assert_eq!(
            persist(&a, &session, &stored).unwrap_err().0,
            StatusCode::UNAUTHORIZED
        );
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=0,is_admin=0 WHERE id=1", [])
            .unwrap();
        assert!(persist(&a, &session, &stored).is_err());
    }
    #[test]
    fn score_rejects_foreign_teams_huge_lists_and_invalid_numbers() {
        let raw = json!({"tournament":"CS2","stage":"final","status":"live","best_of":3,"teams":[{"id":"a","name":"A","score":3,"maps_won":0,"side":"T"},{"id":"b","name":"B","score":4,"maps_won":0,"side":"CT"}],"map":"de_dust2","round":8,"phase":"live","bomb":null,"players":[],"maps":[],"rounds":["a","b"],"events":[]});
        let mut score: Score = serde_json::from_value(raw).unwrap();
        assert!(score.validate().is_ok());
        score.rounds.push("not-a-team".into());
        assert!(score.validate().is_err());
        score.rounds.clear();
        score.events = vec!["event".into(); 51];
        assert!(score.validate().is_err());
        score.events.clear();
        score.teams[1].id = "a".into();
        assert!(score.validate().is_err());
    }
    #[tokio::test]
    async fn replay_counter_survives_restart_and_a_new_event_resets_it() {
        let (_dir, a, headers) = crate::app::tests::fixture();
        a.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET is_admin=1 WHERE id=1", [])
            .unwrap();
        let session = a.session(&headers).unwrap();
        let mut stored = Stored {
            event_id: "original".into(),
            ..Default::default()
        };
        persist(&a, &session, &stored).unwrap();
        a.db.connection
            .lock()
            .unwrap()
            .execute("INSERT INTO watch_feed_state VALUES(1,'original',10)", [])
            .unwrap();
        assert_eq!(
            Watch::load(&a.db, &a.vault)
                .unwrap()
                .live
                .read()
                .unwrap()
                .sequence,
            10
        );
        stored.event_id = "new-event".into();
        persist(&a, &session, &stored).unwrap();
        assert_eq!(
            Watch::load(&a.db, &a.vault)
                .unwrap()
                .live
                .read()
                .unwrap()
                .sequence,
            0
        );
    }
}
