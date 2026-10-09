use crate::{
    app::{Api, App, Error},
    db::{Db, Session, now},
    settings::admin,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use lettre::{AsyncTransport, Message};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    title: String,
    body: String,
    format: String,
    audience: String,
    #[serde(default)]
    recipients: Vec<i64>,
    #[serde(default)]
    email: bool,
    #[serde(default)]
    password: String,
    #[serde(default)]
    request_id: String,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    #[serde(default)]
    before: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preference {
    email_enabled: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirm {
    password: String,
}

const ELIGIBLE: &str = "u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=? AND (?='all' OR u.id IN(SELECT value FROM json_each(?)))";
fn clean_html(value: &str) -> String {
    ammonia::Builder::default()
        .tags(
            [
                "p",
                "br",
                "strong",
                "em",
                "b",
                "i",
                "s",
                "h1",
                "h2",
                "h3",
                "h4",
                "ul",
                "ol",
                "li",
                "blockquote",
                "pre",
                "code",
                "hr",
                "a",
                "img",
                "table",
                "thead",
                "tbody",
                "tr",
                "th",
                "td",
            ]
            .into_iter()
            .collect(),
        )
        .generic_attributes(HashSet::new())
        .tag_attributes(HashMap::from([
            ("a", HashSet::from(["href", "title"])),
            ("img", HashSet::from(["src", "alt", "title"])),
        ]))
        .url_schemes(HashSet::from(["https", "data"]))
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer nofollow"))
        .set_tag_attribute_value("a", "target", "_blank")
        .attribute_filter(|tag, attribute, value| {
            if (tag == "a" && attribute == "href") || (tag == "img" && attribute == "src") {
                if tag == "img" && value.starts_with("data:image/jpeg;base64,") {
                    let mut image = value.to_owned();
                    return crate::site::image(&mut image).ok().map(|_| image.into());
                }
                let url = url::Url::parse(value).ok()?;
                return (url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none())
                .then(|| value.into());
            }
            Some(value.into())
        })
        .clean(value)
        .to_string()
}
impl Draft {
    fn normalize(&mut self) -> Api<String> {
        self.title = self.title.trim().to_owned();
        if self.title.is_empty()
            || self.title.chars().count() > 100
            || self.title.chars().any(char::is_control)
            || self.body.len() > 196608
            || self.body.trim().is_empty()
            || !matches!(self.audience.as_str(), "all" | "selected")
            || self.recipients.len() > 100
            || self.recipients.iter().any(|id| *id <= 0)
            || self.audience == "selected" && self.recipients.is_empty()
        {
            return Err(Error::bad(
                "标题、正文或收件范围无效；正文最多192KiB，指定收件人最多100位",
            ));
        }
        self.recipients.sort_unstable();
        self.recipients.dedup();
        let html = match self.format.as_str() {
            "html" => self.body.clone(),
            "markdown" => {
                let mut out = String::new();
                pulldown_cmark::html::push_html(
                    &mut out,
                    pulldown_cmark::Parser::new_ext(
                        &self.body,
                        pulldown_cmark::Options::ENABLE_TABLES
                            | pulldown_cmark::Options::ENABLE_STRIKETHROUGH,
                    ),
                );
                out
            }
            _ => return Err(Error::bad("信件格式必须为Markdown或HTML")),
        };
        let cleaned = clean_html(&html);
        if cleaned.len() > 196608 || cleaned.trim().is_empty() {
            return Err(Error::bad("安全处理后的正文为空或过大"));
        }
        Ok(cleaned)
    }
    fn ids(&self) -> String {
        serde_json::to_string(&self.recipients).unwrap()
    }
    fn hash(&self, html: &str) -> String {
        hex::encode(Sha256::digest(
            json!([self.title, html, self.audience, self.recipients, self.email]).to_string(),
        ))
    }
}
impl Db {
    fn notice_counts(&self, draft: &Draft) -> anyhow::Result<Value> {
        let c = self.connection.lock().unwrap();
        let (users,emails):(i64,i64)=c.query_row(&format!("SELECT COUNT(*),COALESCE(SUM(COALESCE(p.email_enabled,0)),0) FROM users u LEFT JOIN notice_preferences p ON p.user_id=u.id WHERE {ELIGIBLE}"),params![now(),draft.audience,draft.ids()],|r|Ok((r.get(0)?,r.get(1)?)))?;
        Ok(json!({"recipients":users,"email_recipients":if draft.email{emails}else{0}}))
    }
    fn publish_notice(&self, session: &Session, draft: &Draft, html: &str) -> Api<Value> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction().map_err(anyhow::Error::from)?;
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM users u JOIN sessions s ON s.user_id=u.id WHERE u.id=? AND u.verified=1 AND u.is_admin=1 AND u.banned_until!=-1 AND u.banned_until<=? AND s.hash=? AND s.expires>?)",params![session.user.id,now(),session.hash,now()],|r|r.get(0)).map_err(anyhow::Error::from)?;
        if !valid {
            return Err(Error(StatusCode::UNAUTHORIZED, "管理员登录已失效"));
        }
        let previous: Option<(i64, String)> = tx
            .query_row(
                "SELECT id,request_hash FROM notices WHERE author=? AND request_id=?",
                params![session.user.id, draft.request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(anyhow::Error::from)?;
        if let Some((id, hash)) = previous {
            return if hash == draft.hash(html) {
                Ok(json!({"id":id,"duplicate":true}))
            } else {
                Err(Error(StatusCode::CONFLICT, "重复请求的信件内容不一致"))
            };
        }
        let recent: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM notices WHERE author=? AND created_at>?)",
                params![session.user.id, now() - 60],
                |r| r.get(0),
            )
            .map_err(anyhow::Error::from)?;
        if recent {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "每位管理员每分钟最多发布一封信件",
            ));
        }
        let count: i64 = tx
            .query_row("SELECT COUNT(*) FROM notices", [], |r| r.get(0))
            .map_err(anyhow::Error::from)?;
        let receipts: i64 = tx
            .query_row("SELECT COUNT(*) FROM notice_receipts", [], |r| r.get(0))
            .map_err(anyhow::Error::from)?;
        let eligible: i64 = tx
            .query_row(
                &format!("SELECT COUNT(*) FROM users u WHERE {ELIGIBLE}"),
                params![now(), draft.audience, draft.ids()],
                |r| r.get(0),
            )
            .map_err(anyhow::Error::from)?;
        if eligible == 0 {
            return Err(Error::bad("没有符合条件的已验证、未封禁收件人"));
        }
        if eligible > 10000 || count >= 1000 || receipts + eligible > 200000 {
            return Err(Error(
                StatusCode::CONFLICT,
                "信件存储或收件人数已达到安全上限，请联系站点维护人员",
            ));
        }
        tx.execute("INSERT INTO notices(author,request_id,request_hash,title,html,created_at) VALUES(?,?,?,?,?,?)",params![session.user.id,draft.request_id,draft.hash(html),draft.title,html,now()]).map_err(anyhow::Error::from)?;
        let id = tx.last_insert_rowid();
        tx.execute(&format!("INSERT INTO notice_receipts(notice_id,user_id,mail_state) SELECT ?,u.id,CASE WHEN ? AND COALESCE(p.email_enabled,0)=1 THEN 'pending' ELSE 'none' END FROM users u LEFT JOIN notice_preferences p ON p.user_id=u.id WHERE {ELIGIBLE}"),params![id,draft.email,now(),draft.audience,draft.ids()]).map_err(anyhow::Error::from)?;
        tx.commit().map_err(anyhow::Error::from)?;
        Ok(json!({"id":id,"duplicate":false,"recipients":eligible}))
    }
    fn inbox(&self, owner: i64, before: i64) -> anyhow::Result<Value> {
        let c = self.connection.lock().unwrap();
        let mut q=c.prepare("SELECT n.id,n.title,n.created_at,r.read_at FROM notices n JOIN notice_receipts r ON r.notice_id=n.id WHERE r.user_id=? AND n.withdrawn=0 AND (?=0 OR n.id<?) ORDER BY n.id DESC LIMIT 50")?;
        let items=q.query_map(params![owner,before,before],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"title":r.get::<_,String>(1)?,"created_at":r.get::<_,i64>(2)?,"read":r.get::<_,Option<i64>>(3)?.is_some()})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let unread:i64=c.query_row("SELECT COUNT(*) FROM notice_receipts r JOIN notices n ON n.id=r.notice_id WHERE r.user_id=? AND r.read_at IS NULL AND n.withdrawn=0",[owner],|r|r.get(0))?;
        let enabled: bool = c
            .query_row(
                "SELECT email_enabled FROM notice_preferences WHERE user_id=?",
                [owner],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false);
        Ok(json!({"items":items,"unread":unread,"email_enabled":enabled}))
    }
    fn notice_detail(&self, owner: i64, id: i64) -> Api<Value> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction().map_err(anyhow::Error::from)?;
        let value=tx.query_row("SELECT n.id,n.title,n.html,n.created_at FROM notices n JOIN notice_receipts r ON r.notice_id=n.id WHERE r.user_id=? AND n.id=? AND n.withdrawn=0",params![owner,id],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"title":r.get::<_,String>(1)?,"html":r.get::<_,String>(2)?,"created_at":r.get::<_,i64>(3)?}))).optional().map_err(anyhow::Error::from)?.ok_or(Error(StatusCode::NOT_FOUND,"信件不存在或已撤回"))?;
        tx.execute("UPDATE notice_receipts SET read_at=COALESCE(read_at,?) WHERE user_id=? AND notice_id=?",params![now(),owner,id]).map_err(anyhow::Error::from)?;
        tx.commit().map_err(anyhow::Error::from)?;
        Ok(value)
    }
    fn notice_history(&self) -> anyhow::Result<Value> {
        let c = self.connection.lock().unwrap();
        let mut q=c.prepare("SELECT n.id,n.title,n.created_at,n.withdrawn,COUNT(r.user_id),COALESCE(SUM(r.read_at IS NOT NULL),0),COALESCE(SUM(r.mail_state='sent'),0),COALESCE(SUM(r.mail_state='failed'),0),COALESCE(SUM(r.mail_state IN('pending','sending')),0) FROM notices n LEFT JOIN notice_receipts r ON r.notice_id=n.id GROUP BY n.id ORDER BY n.id DESC LIMIT 50")?;
        Ok(
            json!({"items":q.query_map([],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"title":r.get::<_,String>(1)?,"created_at":r.get::<_,i64>(2)?,"withdrawn":r.get::<_,bool>(3)?,"recipients":r.get::<_,i64>(4)?,"read":r.get::<_,i64>(5)?,"sent":r.get::<_,i64>(6)?,"failed":r.get::<_,i64>(7)?,"pending":r.get::<_,i64>(8)?})))?.collect::<rusqlite::Result<Vec<_>>>()?}),
        )
    }
}
pub async fn preview(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut draft): Json<Draft>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        admin(a, &headers)?;
        let html = draft.normalize()?;
        let mut counts = a.db.notice_counts(&draft)?;
        counts["title"] = json!(draft.title);
        counts["html"] = json!(html);
        counts["smtp_ready"] = json!(a.runtime.read().unwrap().mailer.is_some());
        Ok(Json(counts))
    })
    .await
}
pub async fn publish(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut draft): Json<Draft>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        admin(a, &headers)?;
        let password = Zeroizing::new(std::mem::take(&mut draft.password));
        let session = a.reauthenticate(&headers, &password)?;
        let html = draft.normalize()?;
        if draft.request_id.len() != 36
            || !draft
                .request_id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b'-')
        {
            return Err(Error::bad("发布请求标识无效"));
        }
        if draft.email && a.runtime.read().unwrap().mailer.is_none() {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "尚未配置SMTP，请关闭邮件同步或先配置邮箱",
            ));
        }
        Ok(Json(a.db.publish_notice(&session, &draft, &html)?))
    })
    .await
}
pub async fn history(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a| {
        admin(a, &headers)?;
        Ok(Json(a.db.notice_history()?))
    })
    .await
}
pub async fn list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let owner = a.session(&headers)?.user.id;
        if page.before < 0 {
            return Err(Error::bad("分页参数无效"));
        }
        Ok(Json(a.db.inbox(owner, page.before)?))
    })
    .await
}
pub async fn detail(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(_body): Json<Value>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        let owner = a.session(&headers)?.user.id;
        Ok(Json(a.db.notice_detail(owner, id)?))
    })
    .await
}
pub async fn preferences(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Preference>,
) -> Api<Json<Value>> {
    app.work(move|a|{let owner=a.session(&headers)?.user.id;let c=a.db.connection.lock().unwrap();c.execute("INSERT INTO notice_preferences VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET email_enabled=excluded.email_enabled",params![owner,body.email_enabled]).map_err(anyhow::Error::from)?;if !body.email_enabled{c.execute("UPDATE notice_receipts SET mail_state='cancelled' WHERE user_id=? AND mail_state='pending'",[owner]).map_err(anyhow::Error::from)?;}Ok(Json(json!({"email_enabled":body.email_enabled})))}).await
}
pub async fn withdraw(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<Confirm>,
) -> Api<Json<Value>> {
    app.work(move|a|{admin(a,&headers)?;let session=a.reauthenticate(&headers,&Zeroizing::new(body.password))?;let mut c=a.db.connection.lock().unwrap();let tx=c.transaction().map_err(anyhow::Error::from)?;
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM users u JOIN sessions s ON s.user_id=u.id WHERE u.id=? AND u.is_admin=1 AND u.banned_until!=-1 AND u.banned_until<=? AND s.hash=? AND s.expires>?)",params![session.user.id,now(),session.hash,now()],|r|r.get(0)).map_err(anyhow::Error::from)?;
        if !valid{return Err(Error(StatusCode::UNAUTHORIZED,"管理员登录已失效"));}
        if tx.execute("UPDATE notices SET withdrawn=1 WHERE id=?",[id]).map_err(anyhow::Error::from)?==0{return Err(Error(StatusCode::NOT_FOUND,"信件不存在"));}
        tx.execute("UPDATE notice_receipts SET mail_state='cancelled' WHERE notice_id=? AND mail_state='pending'",[id]).map_err(anyhow::Error::from)?;tx.commit().map_err(anyhow::Error::from)?;
        Ok(Json(json!({"message":"站内信已撤回，待发邮件已取消；正在发送或已发送的邮件无法撤回。"})))}).await
}

struct Delivery {
    id: i64,
    owner: i64,
    email: String,
    title: String,
    html: String,
}
impl Db {
    fn claim_notice_mail(&self) -> anyhow::Result<Option<Delivery>> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("UPDATE notice_receipts SET mail_state='cancelled' WHERE mail_state='pending' AND (NOT EXISTS(SELECT 1 FROM notices n JOIN users a ON a.id=n.author WHERE n.id=notice_receipts.notice_id AND n.withdrawn=0 AND a.is_admin=1 AND a.verified=1 AND a.banned_until!=-1 AND a.banned_until<=?) OR NOT EXISTS(SELECT 1 FROM users u JOIN notice_preferences p ON p.user_id=u.id WHERE u.id=notice_receipts.user_id AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=? AND p.email_enabled=1))",params![now(),now()])?;
        let job=tx.query_row("SELECT n.id,u.id,u.email,n.title,n.html FROM notices n JOIN notice_receipts r ON r.notice_id=n.id JOIN users u ON u.id=r.user_id WHERE r.mail_state='pending' ORDER BY n.id,u.id LIMIT 1",[],|r|Ok(Delivery{id:r.get(0)?,owner:r.get(1)?,email:r.get(2)?,title:r.get(3)?,html:r.get(4)?})).optional()?;
        if let Some(j) = &job {
            tx.execute("UPDATE notice_receipts SET mail_state='sending' WHERE notice_id=? AND user_id=? AND mail_state='pending'",params![j.id,j.owner])?;
        }
        tx.commit()?;
        Ok(job)
    }
}
fn mail_message(
    from: lettre::message::Mailbox,
    job: &Delivery,
    site: &str,
    origin: &str,
) -> Option<Message> {
    let footer = format!(
        "<hr><p>此信来自 {}。请登录 <a href=\"{}/app\">站点信箱</a> 查看通知或关闭更新邮件。</p>",
        ammonia::clean_text(site),
        ammonia::clean_text(origin)
    );
    Message::builder()
        .from(from)
        .to(job.email.parse().ok()?)
        .subject(format!("[{site}] {}", job.title))
        .multipart(lettre::message::MultiPart::alternative_plain_html(
            format!(
                "{}\n\n请登录 {origin}/app 查看完整内容，或在站点信箱关闭更新邮件。",
                job.title
            ),
            format!(
                "<!doctype html><html><body><h1>{}</h1>{}{footer}</body></html>",
                ammonia::clean_text(&job.title),
                job.html
            ),
        ))
        .ok()
}
pub fn start(app: Arc<App>) {
    tokio::spawn(async move {
        // An interrupted SMTP transaction is uncertain: mark failed, never resend blindly.
        loop {
            if app.work(|a|{a.db.connection.lock().unwrap().execute("UPDATE notice_receipts SET mail_state='failed' WHERE mail_state='sending'",[]).map_err(anyhow::Error::from)?;Ok(())}).await.is_ok() {break;}
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let (mailer, site) = {
                let r = app.runtime.read().unwrap();
                (r.mailer.clone(), r.settings.home.site_name.clone())
            };
            let Some(mailer) = mailer else { continue };
            let job = match app.work(|a| Ok(a.db.claim_notice_mail()?)).await {
                Ok(Some(job)) => job,
                _ => continue,
            };
            let ok = if let Some(message) =
                mail_message(mailer.from, &job, &site, &app.config.origin())
            {
                matches!(
                    tokio::time::timeout(Duration::from_secs(20), mailer.transport.send(message))
                        .await,
                    Ok(Ok(_))
                )
            } else {
                false
            };
            let (id, owner) = (job.id, job.owner);
            // Persist completion before claiming another job; a busy DB must not resend SMTP.
            loop {
                if app.work(move|a|{a.db.connection.lock().unwrap().execute("UPDATE notice_receipts SET mail_state=? WHERE notice_id=? AND user_id=? AND mail_state='sending'",params![if ok{"sent"}else{"failed"},id,owner]).map_err(anyhow::Error::from)?;Ok(())}).await.is_ok() {break;}
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html_and_markdown_cannot_run_code_or_rewrite_links() {
        let mut d=Draft{title:"更新".into(),body:"# 新版本\n\n**更新** [链接](https://example.com)\n\n<script>alert(1)</script><img src='data:image/svg+xml,evil' onerror='evil()'><a href='javascript:evil()'>bad</a><iframe src='https://evil.example'></iframe>".into(),format:"markdown".into(),audience:"all".into(),recipients:vec![],email:false,password:String::new(),request_id:String::new()};
        let html = d.normalize().ok().expect("valid notice");
        assert!(html.contains("<h1>新版本</h1>"));
        assert!(html.contains("<strong>更新</strong>"));
        assert!(html.contains("noopener noreferrer nofollow"));
        for bad in [
            "script",
            "onerror",
            "javascript",
            "svg+xml",
            "iframe",
            "evil()",
        ] {
            assert!(!html.contains(bad), "{bad}");
        }
        d.format = "html".into();
        d.body="<h2>标题</h2><img src='https://user:password@example.com/a.jpg'><img src='http://example.com/a.jpg'><input autofocus><style>evil</style>".into();
        let html = d.normalize().ok().expect("valid notice");
        assert!(html.contains("<h2>标题</h2>"));
        assert!(!html.contains("password"));
        assert!(!html.contains("http:"));
        assert!(!html.contains("autofocus"));
        assert!(!html.contains("style"));
    }
    #[test]
    fn subjects_body_targets_and_inline_images_are_bounded() {
        let mut d = Draft {
            title: "bad\r\nBcc: stolen".into(),
            body: "hello".into(),
            format: "html".into(),
            audience: "all".into(),
            recipients: vec![],
            email: false,
            password: String::new(),
            request_id: String::new(),
        };
        assert!(d.normalize().is_err());
        d.title = "更新".into();
        d.audience = "selected".into();
        assert!(d.normalize().is_err());
        d.recipients = vec![1, 1];
        assert!(d.normalize().is_ok());
        assert_eq!(d.recipients, vec![1]);
        d.body = "x".repeat(196609);
        assert!(d.normalize().is_err());
    }
    #[test]
    fn queued_mail_obeys_subscription_revocation_and_audience_snapshot() {
        let db = Db::open(":memory:").unwrap();
        let hash = crate::password::hash("synthetic letters queue password").unwrap();
        let mut users = Vec::new();
        for address in [
            "admin@example.invalid",
            "one@example.invalid",
            "two@example.invalid",
        ] {
            let (id, _, token) = db.register(address, &hash).unwrap();
            db.consume_email_token(&token, "verify", None).unwrap();
            users.push(id);
        }
        db.grant_admin("admin@example.invalid").unwrap();
        let token = db.create_session(users[0], false, &hash).unwrap();
        let session = db.session(&token).unwrap().unwrap();
        db.connection
            .lock()
            .unwrap()
            .execute("INSERT INTO notice_preferences VALUES(?,1)", [users[1]])
            .unwrap();
        let draft = Draft {
            title: "更新".into(),
            body: "safe".into(),
            format: "html".into(),
            audience: "all".into(),
            recipients: vec![],
            email: true,
            password: String::new(),
            request_id: "queue-test".into(),
        };
        let result = db
            .publish_notice(&session, &draft, "<p>safe</p>")
            .ok()
            .expect("authorized publish");
        let id = result["id"].as_i64().unwrap();
        let (later, _, verify) = db.register("later@example.invalid", &hash).unwrap();
        db.consume_email_token(&verify, "verify", None).unwrap();
        assert_eq!(db.inbox(later, 0).unwrap()["unread"], 0);
        let job = db.claim_notice_mail().unwrap().unwrap();
        assert_eq!(job.owner, users[1]);
        assert_eq!(job.email, "one@example.invalid");
        assert!(db.claim_notice_mail().unwrap().is_none());
        // Never reclaim a transaction which may already have been accepted by SMTP.
        db.connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE notice_receipts SET mail_state='failed' WHERE mail_state='sending'",
                [],
            )
            .unwrap();
        assert!(db.claim_notice_mail().unwrap().is_none());
        {
            let c = db.connection.lock().unwrap();
            c.execute(
                "UPDATE notice_receipts SET mail_state='pending' WHERE notice_id=? AND user_id=?",
                params![id, users[1]],
            )
            .unwrap();
            c.execute(
                "UPDATE notice_preferences SET email_enabled=0 WHERE user_id=?",
                [users[1]],
            )
            .unwrap();
        }
        assert!(db.claim_notice_mail().unwrap().is_none());
        {
            let c = db.connection.lock().unwrap();
            c.execute(
                "UPDATE notice_preferences SET email_enabled=1 WHERE user_id=?",
                [users[1]],
            )
            .unwrap();
            c.execute(
                "UPDATE notice_receipts SET mail_state='pending' WHERE notice_id=? AND user_id=?",
                params![id, users[1]],
            )
            .unwrap();
            c.execute("UPDATE users SET is_admin=0 WHERE id=?", [users[0]])
                .unwrap();
        }
        assert!(db.claim_notice_mail().unwrap().is_none());
        assert_eq!(
            db.publish_notice(&session, &draft, "<p>safe</p>")
                .err()
                .unwrap()
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    #[test]
    fn message_has_a_single_recipient_and_escaped_title() {
        let job = Delivery {
            id: 1,
            owner: 2,
            email: "only@example.invalid".into(),
            title: "<更新> & 安全".into(),
            html: "<p>安全正文</p>".into(),
        };
        let message = mail_message(
            "site@example.invalid".parse().unwrap(),
            &job,
            "测试站点",
            "https://example.invalid",
        )
        .unwrap();
        assert_eq!(message.envelope().to().len(), 1);
        assert_eq!(
            message.envelope().to()[0].to_string(),
            "only@example.invalid"
        );
        let formatted = String::from_utf8(message.formatted()).unwrap();
        assert!(!formatted.contains("Bcc:"));
        assert!(!formatted.contains("Cc:"));
        assert!(formatted.contains("multipart/alternative"));
        assert!(formatted.contains("text/html"));
        assert!(formatted.contains("text/plain"));
    }
}
