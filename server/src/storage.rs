//! Private, fixed-prefix S3 objects. The browser encrypts first; Vault adds at-rest protection.
use crate::{
    app::{Api, App, Error},
    settings::admin,
};
use anyhow::{Result, bail};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_CIPHERTEXT: usize = 384 * 1024;
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
    #[serde(default)]
    pub retention_days: u32,
}
impl Drop for Storage {
    fn drop(&mut self) {
        self.access_key.zeroize();
        self.secret_key.zeroize();
    }
}
impl Storage {
    fn normalize(&mut self) {
        self.endpoint = self.endpoint.trim().to_owned();
        self.bucket = self.bucket.trim().to_owned();
        self.region = self.region.trim().to_owned();
        if let Ok(url) = url::Url::parse(&self.endpoint) {
            if self.region.is_empty() && url.host_str().is_some_and(|h| h.ends_with(".rains3.com"))
            {
                self.region = "us-east-1".into();
            }
            if url.path() == "/" {
                self.endpoint = url.to_string().trim_end_matches('/').to_owned();
            }
        }
    }
    fn same_bucket(&self, other: &Self) -> bool {
        self.bucket == other.bucket
            && match (
                url::Url::parse(&self.endpoint),
                url::Url::parse(&other.endpoint),
            ) {
                (Ok(a), Ok(b)) => a == b,
                _ => self.endpoint == other.endpoint,
            }
    }
    pub fn validate(&self) -> Result<()> {
        if self.endpoint.is_empty() && !self.enabled {
            return Ok(());
        }
        let url = url::Url::parse(&self.endpoint)?;
        if url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !(3..=63).contains(&self.bucket.len())
            || !self
                .bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !self.bucket.as_bytes()[0].is_ascii_alphanumeric()
            || self.bucket.ends_with('-')
            || self.region.is_empty()
            || self.region.len() > 64
            || !self
                .region
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || !(1..=30).contains(&self.retention_days)
            || self.access_key.is_empty()
            || self.access_key.len() > 256
            || self.secret_key.len() < 16
            || self.secret_key.len() > 1024
            || self.access_key.chars().any(char::is_control)
            || self.secret_key.chars().any(char::is_control)
        {
            bail!("对象存储配置无效");
        }
        let host = url.host_str().unwrap().trim_matches(['[', ']']);
        if let Ok(ip) = host.parse::<std::net::IpAddr>()
            && !tsclientlib::resolver::is_public_addr(&ip)
        {
            bail!("存储端点必须为公网");
        }
        Ok(())
    }
    pub fn view(&self) -> Value {
        json!({"enabled":self.enabled,"endpoint":self.endpoint,"bucket":self.bucket,"region":self.region,
            "retention_days":if self.retention_days == 0 {7} else {self.retention_days},"credentials_set":!self.secret_key.is_empty()})
    }
}
fn mac(key: &[u8], text: &str) -> Vec<u8> {
    let mut h = Hmac::<Sha256>::new_from_slice(key).unwrap();
    h.update(text.as_bytes());
    h.finalize().into_bytes().to_vec()
}
fn authorization(
    config: &Storage,
    method: &str,
    path: &str,
    host: &str,
    date: &str,
    hash: &str,
) -> String {
    let scope = format!("{}/{}/s3/aws4_request", &date[..8], config.region);
    let headers = format!("host:{host}\nx-amz-content-sha256:{hash}\nx-amz-date:{date}\n");
    let signed = "host;x-amz-content-sha256;x-amz-date";
    let canonical = format!("{method}\n{path}\n\n{headers}\n{signed}\n{hash}");
    let string = format!(
        "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let root = Zeroizing::new(format!("AWS4{}", config.secret_key));
    let kd = Zeroizing::new(mac(root.as_bytes(), &date[..8]));
    let kr = Zeroizing::new(mac(&kd, &config.region));
    let ks = Zeroizing::new(mac(&kr, "s3"));
    let signing = Zeroizing::new(mac(&ks, "aws4_request"));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={}",
        config.access_key,
        hex::encode(mac(&signing, &string))
    )
}
// Only allowlisted status/code pairs cross the API boundary; never echo an S3 response body.
#[derive(Debug)]
struct ObjectError {
    status: u16,
    code: &'static str,
}
impl std::fmt::Display for ObjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "S3 HTTP {} {}", self.status, self.code)
    }
}
impl std::error::Error for ObjectError {}
pub fn failure_message(error: &anyhow::Error) -> &'static str {
    match error.downcast_ref::<ObjectError>().map(|e| e.code) {
        Some("AccessDenied") => {
            "对象存储拒绝访问（AccessDenied）：请核对桶名、密钥的读写删除权限和IP白名单"
        }
        Some("InvalidAccessKeyId" | "InvalidToken" | "ExpiredToken") => {
            "对象存储访问密钥无效或已过期，请重新配置"
        }
        Some("SignatureDoesNotMatch" | "AuthorizationHeaderMalformed") => {
            "对象存储签名不匹配，请核对Region和Access Key对应的Secret Key"
        }
        Some("NoSuchBucket") => "对象存储桶不存在，请填写控制台中的准确桶名",
        Some("RequestTimeTooSkewed") => "对象存储拒绝签名：部署服务器时间偏差过大，请校准时间",
        Some("NoSuchKey") => "临时对象不存在或已被生命周期清理",
        _ => "对象存储请求失败，请检查HTTPS端点、网络和访问配置",
    }
}
pub fn missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<ObjectError>()
        .is_some_and(|e| e.status == 404 && e.code == "NoSuchKey")
}
pub async fn remove(config: &Storage, key: &str) -> Result<()> {
    match object(config, "DELETE", key, vec![]).await {
        Ok(_) => Ok(()),
        Err(error) => {
            // A failed DELETE must not leave an expired index stuck when S3 confirms absence.
            if object(config, "GET", key, vec![])
                .await
                .as_ref()
                .err()
                .is_some_and(missing)
            {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}
pub async fn object(config: &Storage, method: &str, key: &str, body: Vec<u8>) -> Result<Vec<u8>> {
    // Neither a user-supplied URL nor a pre-signed URL crosses this boundary.
    let result = tokio::time::timeout(Duration::from_secs(18), async {
        config.validate()?;
        if key.len() != 64
            || !key.bytes().all(|b| b.is_ascii_hexdigit())
            || !["GET", "PUT", "DELETE"].contains(&method)
            || body.len() > MAX_CIPHERTEXT + 29
        {
            bail!("对象无效");
        }
        let url = url::Url::parse(&config.endpoint)?;
        let host = url.host_str().unwrap().trim_matches(['[', ']']).to_owned();
        let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), 443))
            .await?
            .take(17)
            .collect();
        if addresses.is_empty()
            || addresses.len() > 16
            || addresses
                .iter()
                .any(|a| !tsclientlib::resolver::is_public_addr(&a.ip()))
        {
            bail!("存储端点必须为公网");
        }
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(&host, &addresses)
            .timeout(Duration::from_secs(15))
            .build()?;
        let date =
            time::OffsetDateTime::now_utc().format(&time::format_description::parse_borrowed::<
                2,
            >(
                "[year][month][day]T[hour][minute][second]Z",
            )?)?;
        request_object(config, &client, url, method, key, body, &date).await
    })
    .await;
    result
        .unwrap_or_else(|_| Err(anyhow::anyhow!("存储请求超时")))
        .map_err(|e| {
            if e.is::<ObjectError>() {
                e
            } else {
                anyhow::anyhow!("对象存储不可用，请检查站点存储配置")
            }
        })
}
// Called only after object() validates and pins the configured HTTPS endpoint.
async fn request_object(
    config: &Storage,
    client: &reqwest::Client,
    mut url: url::Url,
    method: &str,
    key: &str,
    body: Vec<u8>,
    date: &str,
) -> Result<Vec<u8>> {
    let path = format!("/{}/webts-temporary/v1/{key}", config.bucket);
    url.set_path(&path);
    let hash = hex::encode(Sha256::digest(&body));
    let host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap()),
        None => url.host_str().unwrap().to_owned(),
    };
    let auth = authorization(config, method, &path, &host, date, &hash);
    let mut response = client
        .request(method.parse()?, url)
        .header("x-amz-date", date)
        .header("x-amz-content-sha256", hash)
        .header("authorization", auth)
        .body(body)
        .send()
        .await?;
    if method == "DELETE" && response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len().saturating_add(chunk.len()) > 4096 {
                break;
            }
            body.extend_from_slice(&chunk);
        }
        let text = String::from_utf8_lossy(&body);
        let value = text
            .split_once("<Code>")
            .and_then(|(_, v)| v.split_once("</Code>"))
            .map(|(v, _)| v.trim());
        let code = match value {
            Some("AccessDenied") => "AccessDenied",
            Some("InvalidAccessKeyId") => "InvalidAccessKeyId",
            Some("SignatureDoesNotMatch") => "SignatureDoesNotMatch",
            Some("AuthorizationHeaderMalformed") => "AuthorizationHeaderMalformed",
            Some("NoSuchBucket") => "NoSuchBucket",
            Some("RequestTimeTooSkewed") => "RequestTimeTooSkewed",
            Some("NoSuchKey") => "NoSuchKey",
            Some("ExpiredToken") => "ExpiredToken",
            Some("InvalidToken") => "InvalidToken",
            _ => "UnclassifiedError",
        };
        return Err(ObjectError { status, code }.into());
    }
    if method != "GET" {
        return Ok(Vec::new());
    }
    let max = MAX_CIPHERTEXT + 29;
    if response.content_length().is_some_and(|n| n > max as u64) {
        bail!("对象过大");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > max {
            bail!("对象过大");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    password: String,
    storage: Storage,
}
pub async fn get(State(app): State<Arc<App>>, headers: HeaderMap) -> Api<Json<Value>> {
    app.work(move |a| {
        admin(a, &headers)?;
        let mut view = a.runtime.read().unwrap().settings.storage.view();
        let db = a.db.connection.lock().unwrap();
        let (total, pending, expired): (i64,i64,i64) = db.query_row("SELECT count(*),COALESCE(sum(ready=0),0),COALESCE(sum(expires<=?),0) FROM friend_messages", [crate::db::now()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(anyhow::Error::from)?;
        let probes: i64 = db.query_row("SELECT count(*) FROM storage_probes", [], |r| r.get(0)).map_err(anyhow::Error::from)?;
        view["objects"] = json!({"total":total,"pending":pending,"expired":expired,"probes":probes});
        Ok(Json(view))
    }).await
}
pub async fn save(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Update>,
) -> Api<Json<Value>> {
    if body.password.len() > 128 {
        return Err(Error::bad("密码输入超出限制"));
    }
    app.work(move |a| {
        admin(a, &headers)?; let session = a.reauthenticate(&headers, &Zeroizing::new(body.password))?;
        body.storage.normalize();
        let mut runtime = a.runtime.write().unwrap(); let old = &runtime.settings.storage;
        if body.storage.access_key.is_empty() { body.storage.access_key = old.access_key.clone(); }
        if body.storage.secret_key.is_empty() {
            if url::Url::parse(&body.storage.endpoint).ok() != url::Url::parse(&old.endpoint).ok() || body.storage.access_key != old.access_key { return Err(Error::bad("更换端点或Access Key时需要填写新Secret Key")); }
            body.storage.secret_key = old.secret_key.clone();
        }
        body.storage.validate().map_err(|_| Error::bad("仅支持公网HTTPS443的S3兼容端点；检查桶名、区域、密钥和1–30天保留期"))?;
        let _migration_guard = if !body.storage.same_bucket(old) {
            let permit = a.friend_objects.clone().try_acquire_many_owned(2).map_err(|_| Error(StatusCode::CONFLICT,"临时对象正在传输或检测，请稍后再迁移存储桶"))?;
            if a.db.connection.lock().unwrap().query_row("SELECT (SELECT count(*) FROM friend_messages)+(SELECT count(*) FROM storage_probes)", [], |r| r.get::<_,i64>(0)).map_err(anyhow::Error::from)? > 0 {
                return Err(Error(StatusCode::CONFLICT, "仍有临时对象，清理完成前不能迁移存储桶；同桶Region修正、关闭新发送和凭据轮换不受此限制"));
            }
            Some(permit)
        } else { None };
        let mut updated = runtime.settings.clone(); updated.storage = body.storage;
        let next = crate::settings::Runtime::new(updated)?;
        let bytes = Zeroizing::new(serde_json::to_vec(&next.settings).map_err(anyhow::Error::from)?);
        let record = a.vault.seal(0, "site-settings", "v1", &bytes)?;
        a.db.save_settings(session.user.id, &session.hash, &record)?;
        *runtime = next;
        Ok(Json(json!({"message":"存储设置已热加载。请在桶中关闭公开访问，并为webts-temporary/配置生命周期删除和旧版本清理。"})))
    }).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Test {
    password: String,
}
pub async fn test(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Test>,
) -> Api<Json<Value>> {
    if body.password.len() > 128 {
        return Err(Error::bad("密码输入超出限制"));
    }
    let _permit = app
        .friend_objects
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            Error(
                StatusCode::TOO_MANY_REQUESTS,
                "存储检测或传输正在进行，请稍后重试",
            )
        })?;
    let (config, key, payload) = app
        .work(move |a| {
            admin(a, &headers)?;
            let s = a.reauthenticate(&headers, &Zeroizing::new(body.password))?;
            let config = a.runtime.read().unwrap().settings.storage.clone();
            config
                .validate()
                .map_err(|_| Error::bad("请先保存完整的S3配置，再检测"))?;
            if config.endpoint.is_empty() {
                return Err(Error::bad("请先保存完整的S3配置，再检测"));
            }
            let key = crate::vault::token()?;
            let db = a.db.connection.lock().unwrap();
            let count: i64 = db
                .query_row("SELECT count(*) FROM storage_probes", [], |r| r.get(0))
                .map_err(anyhow::Error::from)?;
            if count >= 3 {
                return Err(Error(
                    StatusCode::CONFLICT,
                    "仍有待清理的检测对象，请修复删除权限后再检测",
                ));
            }
            db.execute(
                "INSERT INTO storage_probes VALUES(?,?)",
                rusqlite::params![key, crate::db::now()],
            )
            .map_err(anyhow::Error::from)?;
            let payload = a.vault.seal(
                s.user.id,
                "storage-probe",
                &key,
                b"WebTS synthetic encrypted storage diagnostic",
            )?;
            Ok((config, key, payload))
        })
        .await?;
    let put = object(&config, "PUT", &key, payload.clone()).await;
    let write_rejected = put.as_ref().err().is_some_and(|e| e.is::<ObjectError>());
    let read = if put.is_ok() {
        Some(object(&config, "GET", &key, vec![]).await)
    } else {
        None
    };
    let delete = object(&config, "DELETE", &key, vec![]).await;
    let cleanup = delete.is_ok() || write_rejected;
    if cleanup {
        let key = key.clone();
        app.work(move |a| {
            a.db.connection
                .lock()
                .unwrap()
                .execute("DELETE FROM storage_probes WHERE object_key=?", [key])
                .map_err(anyhow::Error::from)?;
            Ok(())
        })
        .await?;
    }
    let same = read
        .as_ref()
        .is_some_and(|r| r.as_ref().is_ok_and(|v| v == &payload));
    let stage = |name: &str, result: &Result<Vec<u8>>| json!({"stage":name,"ok":result.is_ok(),"status":result.as_ref().err().and_then(|e| e.downcast_ref::<ObjectError>()).map(|e| e.status),"code":result.as_ref().err().and_then(|e| e.downcast_ref::<ObjectError>()).map(|e| e.code),"message":result.as_ref().err().map(failure_message)});
    let mut stages = vec![stage("上传", &put)];
    if let Some(read) = &read {
        let mut result = stage("读取", read);
        if read.is_ok() && !same {
            result["ok"] = json!(false);
            result["message"] = json!("读回的密文与测试文件不一致，请检查对象存储服务");
        }
        stages.push(result);
    }
    stages.push(stage("删除", &delete));
    Ok(Json(
        json!({"ok":put.is_ok() && same && delete.is_ok(),"stages":stages,"roundtrip_match":same,"cleanup_pending":!cleanup,"message":if put.is_ok() && same && delete.is_ok() {"加密测试文件上传、读取和删除均通过。"} else {"检测未通过，请按各阶段提示检查。检测不读取或修改用户消息。"}}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_canonical_signature_matches_independent_reference() {
        let c = Storage {
            enabled: false,
            endpoint: String::new(),
            bucket: String::new(),
            retention_days: 0,
            access_key: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            region: "us-east-1".into(),
        };
        let auth = authorization(
            &c,
            "GET",
            "/test.txt",
            "examplebucket.s3.amazonaws.com",
            "20130524T000000Z",
            &hex::encode(Sha256::digest(b"")),
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=df548e2ce037944d03f3e68682813b093763996d597cf890ca3d9037fd231eb4"
        );
    }
    #[tokio::test]
    async fn fixed_prefix_signed_put_get_delete_and_oversized_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let c = Storage {
            enabled: false,
            endpoint: String::new(),
            retention_days: 0,
            bucket: "fixture-bucket".into(),
            region: "us-east-1".into(),
            access_key: "synthetic-key".into(),
            secret_key: "synthetic secret only".into(),
        };
        let check = c.clone();
        let key = "a".repeat(64);
        let object_key = key.clone();
        let date = "20261010T000000Z";
        let payload = crate::vault::Vault::new([4; 32])
            .seal(1, "fixture", "2", b"already end-to-end encrypted envelope")
            .unwrap();
        let expected = payload.clone();
        let server = tokio::spawn(async move {
            let mut stored = Vec::new();
            for step in 0..6 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut input = Vec::new();
                let mut buffer = [0; 1024];
                let header_end = loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    input.extend_from_slice(&buffer[..n]);
                    assert!(input.len() < 8192);
                    if let Some(i) = input.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let headers = String::from_utf8(input[..header_end].to_vec()).unwrap();
                assert!(
                    !headers.to_ascii_lowercase().contains("content-type:"),
                    "optional unsigned Content-Type breaks the configured S3-compatible endpoint"
                );
                let method = if step == 0 {
                    "PUT"
                } else if step == 2 {
                    "DELETE"
                } else {
                    "GET"
                };
                let path = format!("/fixture-bucket/webts-temporary/v1/{object_key}");
                assert!(headers.starts_with(&format!("{method} {path} HTTP/1.1\r\n")));
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|v| v.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while input.len() - header_end < length {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    input.extend_from_slice(&buffer[..n]);
                }
                let body = &input[header_end..header_end + length];
                let hash = hex::encode(Sha256::digest(body));
                let auth = authorization(&check, method, &path, &address.to_string(), date, &hash);
                assert!(headers.contains(&format!("authorization: {auth}\r\n")));
                assert!(headers.contains(&format!("x-amz-content-sha256: {hash}\r\n")));
                let (status, response, declared) = match step {
                    0 => {
                        stored = body.to_vec();
                        assert_eq!(stored, expected);
                        ("200 OK", Vec::new(), 0)
                    }
                    1 => ("200 OK", stored.clone(), stored.len()),
                    2 => {
                        stored.clear();
                        ("404 Not Found", Vec::new(), 0)
                    }
                    3 => ("200 OK", Vec::new(), MAX_CIPHERTEXT + 30),
                    4 => {
                        let error=b"<Error><Code>AccessDenied</Code><Message>private-provider-response-do-not-echo</Message></Error>".to_vec();
                        ("403 Forbidden", error.clone(), error.len())
                    }
                    _ => {
                        let error = b"<Error><Code>NoSuchKey</Code></Error>".to_vec();
                        ("404 Not Found", error.clone(), error.len())
                    }
                };
                socket.write_all(format!("HTTP/1.1 {status}\r\ncontent-length: {declared}\r\nconnection: close\r\n\r\n").as_bytes()).await.unwrap();
                socket.write_all(&response).await.unwrap();
            }
        });
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let url = url::Url::parse(&format!("http://{address}")).unwrap();
        request_object(&c, &client, url.clone(), "PUT", &key, payload.clone(), date)
            .await
            .unwrap();
        assert_eq!(
            request_object(&c, &client, url.clone(), "GET", &key, vec![], date)
                .await
                .unwrap(),
            payload
        );
        request_object(&c, &client, url.clone(), "DELETE", &key, vec![], date)
            .await
            .unwrap();
        assert!(
            request_object(&c, &client, url.clone(), "GET", &key, vec![], date)
                .await
                .is_err()
        );
        let error = request_object(&c, &client, url.clone(), "GET", &key, vec![], date)
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<ObjectError>().unwrap().code,
            "AccessDenied"
        );
        assert!(failure_message(&error).contains("AccessDenied"));
        assert!(!error.to_string().contains("private-provider-response"));
        assert!(!failure_message(&error).contains("private-provider-response"));
        let absent = request_object(&c, &client, url, "GET", &key, vec![], date)
            .await
            .unwrap_err();
        assert!(missing(&absent));
        assert!(!missing(&error));
        server.await.unwrap();
        // The test-only local transport does not weaken the production endpoint validator.
        let mut rejected = c.clone();
        rejected.endpoint = format!("http://{address}");
        assert!(rejected.validate().is_err());
    }
    #[test]
    fn credentials_paths_and_retention_are_restricted() {
        let good = Storage {
            enabled: true,
            endpoint: "https://s3.example.com".into(),
            bucket: "webts-private".into(),
            region: "us-east-1".into(),
            access_key: "fixture".into(),
            secret_key: "synthetic secret only".into(),
            retention_days: 7,
        };
        assert!(good.validate().is_ok());
        let mut rainyun = good.clone();
        rainyun.endpoint = " https://cn-sy1.rains3.com/ ".into();
        rainyun.region = String::new();
        rainyun.normalize();
        assert_eq!(rainyun.region, "us-east-1");
        assert!(rainyun.validate().is_ok());
        let mut signing_correction = rainyun.clone();
        signing_correction.region = "another-region".into();
        signing_correction.endpoint.push('/');
        assert!(rainyun.same_bucket(&signing_correction));
        signing_correction.bucket = "another-bucket".into();
        assert!(!rainyun.same_bucket(&signing_correction));
        for endpoint in [
            "http://s3.example.com",
            "https://127.0.0.1",
            "https://169.254.169.254",
            "https://user:pass@s3.example.com",
            "https://s3.example.com/path",
            "https://s3.example.com:8443",
        ] {
            let mut c = good.clone();
            c.endpoint = endpoint.into();
            assert!(c.validate().is_err());
        }
        let mut bad = good.clone();
        bad.bucket = "../escape".into();
        assert!(bad.validate().is_err());
        bad = good.clone();
        bad.retention_days = 31;
        assert!(bad.validate().is_err());
        let auth = authorization(
            &good,
            "PUT",
            "/webts-private/webts-temporary/v1/0123",
            "s3.example.com",
            "20261010T000000Z",
            &hex::encode(Sha256::digest(b"ciphertext")),
        );
        assert!(auth.contains("20261010/us-east-1/s3/aws4_request"));
        assert!(!auth.contains(&good.secret_key));
        assert_ne!(
            auth,
            authorization(
                &good,
                "GET",
                "/webts-private/webts-temporary/v1/0123",
                "s3.example.com",
                "20261010T000000Z",
                &hex::encode(Sha256::digest(b"ciphertext"))
            )
        );
    }
}
