//! Only images published by the connected TS server may be fetched.
use anyhow::{Result, bail};
use std::{net::SocketAddr, time::Duration};

fn banner_url(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value).map_err(|_| anyhow::anyhow!("服务器图片地址无效"))?;
    if value.len() > 2048
        || value.chars().any(char::is_control)
        || !matches!(
            (url.scheme(), url.port_or_known_default()),
            ("https", Some(443)) | ("http", Some(80))
        )
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        bail!("服务器图片仅支持公网 HTTP/HTTPS 默认端口");
    }
    Ok(url)
}
fn redirect_url(current: &url::Url, location: &str) -> Result<url::Url> {
    if location.len() > 2048 || location.chars().any(char::is_control) {
        bail!("图片跳转地址无效");
    }
    let next = banner_url(
        current
            .join(location)
            .map_err(|_| anyhow::anyhow!("图片跳转地址无效"))?
            .as_str(),
    )?;
    if current.scheme() == "https" && next.scheme() != "https" {
        bail!("禁止 HTTPS 图片跳转到 HTTP");
    }
    Ok(next)
}
fn public_addresses(addresses: &[SocketAddr]) -> bool {
    !addresses.is_empty()
        && addresses.len() <= 16
        && addresses
            .iter()
            .all(|a| tsclientlib::resolver::is_public_addr(&a.ip()))
}
pub async fn download(value: &str, max: usize) -> Result<Vec<u8>> {
    download_bounded(value, max, false).await
}
pub async fn download_page(value: &str) -> Result<Vec<u8>> {
    download_bounded(value, 256 * 1024, true).await
}
pub async fn download_json(value: &str) -> Result<Vec<u8>> {
    let bytes = download_bounded(value, 64 * 1024, false).await?;
    serde_json::from_slice::<serde_json::Value>(&bytes)?;
    Ok(bytes)
}
async fn download_bounded(value: &str, max: usize, html: bool) -> Result<Vec<u8>> {
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let mut url = banner_url(value)?;
        let _ = rustls::crypto::ring::default_provider().install_default();
        for hop in 0..=3 {
            let host = url.host_str().unwrap().trim_matches(['[', ']']);
            let port = url.port_or_known_default().unwrap();
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| anyhow::anyhow!("图床域名解析失败"))?
                .take(17)
                .collect();
            if !public_addresses(&addresses) {
                bail!("图片地址不属于公网");
            }
            // Revalidate and pin every hop; never let reqwest resolve a redirect itself.
            let client = reqwest::Client::builder()
                .no_proxy()
                .user_agent(concat!("WebTS/", env!("CARGO_PKG_VERSION")))
                .redirect(reqwest::redirect::Policy::none())
                .resolve_to_addrs(host, &addresses)
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(25))
                .build()
                .map_err(|_| anyhow::anyhow!("图床连接初始化失败"))?;
            let mut response = client
                .get(url.clone())
                .send()
                .await
                .map_err(|_| anyhow::anyhow!("图床连接失败：请检查证书、网络和访问限制"))?;
            if response.status().is_redirection() {
                if hop == 3 {
                    bail!("图片跳转次数过多");
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .ok_or_else(|| anyhow::anyhow!("图片跳转缺少地址"))?
                    .to_str()
                    .map_err(|_| anyhow::anyhow!("图片跳转地址无效"))?;
                url = redirect_url(&url, location)?;
                continue;
            }
            if !response.status().is_success() {
                bail!("图床拒绝图片读取（HTTP {}）", response.status().as_u16());
            }
            if html {
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                if !matches!(content_type.as_str(), "text/html" | "application/xhtml+xml") {
                    bail!("地址没有返回公开网页；不读取视频或其他文件");
                }
            }
            if response.content_length().is_some_and(|n| n > max as u64) {
                bail!("图片超过站点限制");
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| anyhow::anyhow!("图床图片传输中断"))?
            {
                if bytes.len().saturating_add(chunk.len()) > max {
                    bail!("图片超过站点限制");
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok(bytes);
        }
        bail!("图片跳转次数过多")
    })
    .await;
    // Never expose upstream URLs (possibly containing tokens) or response bodies.
    result.unwrap_or_else(|_| Err(anyhow::anyhow!("服务器图片读取超时")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn banner_urls_reject_credentials_protocols_ports_and_fragments() {
        assert!(banner_url("https://images.example.com/banner.png").is_ok());
        assert!(banner_url("http://images.example.com/banner.png").is_ok());
        for value in [
            "http://example.com:443/a",
            "https://example.com:80/a",
            "https://user:pass@example.com/a",
            "https://example.com:8443/a",
            "file:///a",
            "https://example.com/a#b",
            "https://example.com/a\n",
        ] {
            assert!(banner_url(value).is_err(), "{value}");
        }
    }
    #[test]
    fn image_redirects_preserve_transport_and_validate_each_url() {
        let https = banner_url("https://example.com/images/a.png").unwrap();
        assert_eq!(
            redirect_url(&https, "../b.png").unwrap().as_str(),
            "https://example.com/b.png"
        );
        assert!(redirect_url(&https, "https://cdn.example.com/b.png").is_ok());
        let http = banner_url("http://example.com/a.png").unwrap();
        assert!(redirect_url(&http, "https://cdn.example.com/b.png").is_ok());
        for target in [
            "http://cdn.example.com/a",
            "https://u:p@cdn.example.com/a",
            "file:///a",
            "https://example.com:8443/a",
            "/a#b",
            "/a\r\nX-Test: yes",
        ] {
            assert!(redirect_url(&https, target).is_err(), "{target}");
        }
    }
    #[test]
    fn every_dns_candidate_must_be_public_and_bounded() {
        let public = "1.1.1.1:443".parse().unwrap();
        assert!(public_addresses(&[public]));
        assert!(!public_addresses(&[]));
        assert!(!public_addresses(&vec![public; 17]));
        for private in [
            "127.0.0.1:443",
            "10.0.0.1:443",
            "169.254.169.254:443",
            "[::1]:443",
            "[::ffff:127.0.0.1]:443",
        ] {
            assert!(!public_addresses(&[public, private.parse().unwrap()]));
        }
        let next = redirect_url(
            &banner_url("https://example.com/a").unwrap(),
            "https://127.0.0.1/a",
        )
        .unwrap();
        let address = SocketAddr::new(next.host_str().unwrap().parse().unwrap(), 443);
        assert!(!public_addresses(&[address]));
    }
    #[tokio::test]
    async fn banner_never_reaches_loopback_metadata_or_mapped_addresses() {
        for value in [
            "https://127.0.0.1/a",
            "https://[::1]/a",
            "https://169.254.169.254/a",
            "https://[::ffff:127.0.0.1]/a",
        ] {
            assert!(download(value, 1024).await.is_err());
        }
    }
    #[tokio::test]
    async fn image_errors_do_not_disclose_urls_or_credentials() {
        for value in [
            "https://user:secret@images.example.com/a?token=private",
            "file:///private/token",
            "https://127.0.0.1/a?token=private",
        ] {
            let error = download(value, 1024).await.unwrap_err().to_string();
            assert!(
                !error.contains("private") && !error.contains("secret") && !error.contains("http")
            );
        }
    }
}
