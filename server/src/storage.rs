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
        .map_err(|_| anyhow::anyhow!("对象存储不可用，请检查站点存储配置"))
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
        .header("content-type", "application/octet-stream")
        .body(body)
        .send()
        .await?;
    if method == "DELETE" && response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    if !response.status().is_success() {
        bail!("存储请求失败");
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
    admin(&app, &headers)?;
    Ok(Json(app.runtime.read().unwrap().settings.storage.view()))
}
pub async fn save(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Update>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        admin(a, &headers)?; let session = a.reauthenticate(&headers, &Zeroizing::new(body.password))?;
        let mut runtime = a.runtime.write().unwrap(); let old = &runtime.settings.storage;
        if body.storage.access_key.is_empty() { body.storage.access_key = old.access_key.clone(); }
        if body.storage.secret_key.is_empty() {
            if body.storage.endpoint != old.endpoint || body.storage.access_key != old.access_key { return Err(Error::bad("更换端点或Access Key时需要填写新Secret Key")); }
            body.storage.secret_key = old.secret_key.clone();
        }
        body.storage.validate().map_err(|_| Error::bad("仅支持公网HTTPS443的S3兼容端点；检查桶名、区域、密钥和1–30天保留期"))?;
        if (body.storage.endpoint != old.endpoint || body.storage.bucket != old.bucket || body.storage.region != old.region)
            && a.db.connection.lock().unwrap().query_row("SELECT count(*) FROM friend_messages", [], |r| r.get::<_,i64>(0)).map_err(anyhow::Error::from)? > 0 {
            return Err(Error(StatusCode::CONFLICT, "仍有临时对象，清理完成前不能迁移存储桶；可以关闭新发送或轮换同桶凭据"));
        }
        let mut updated = runtime.settings.clone(); updated.storage = body.storage;
        let next = crate::settings::Runtime::new(updated)?;
        let bytes = Zeroizing::new(serde_json::to_vec(&next.settings).map_err(anyhow::Error::from)?);
        let record = a.vault.seal(0, "site-settings", "v1", &bytes)?;
        a.db.save_settings(session.user.id, &session.hash, &record)?;
        *runtime = next;
        Ok(Json(json!({"message":"存储设置已热加载。请在桶中关闭公开访问，并为webts-temporary/配置生命周期删除和旧版本清理。"})))
    }).await
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
            for step in 0..4 {
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
                    _ => ("200 OK", Vec::new(), MAX_CIPHERTEXT + 30),
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
            request_object(&c, &client, url, "GET", &key, vec![], date)
                .await
                .is_err()
        );
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
