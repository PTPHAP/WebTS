//! Only the banner published by the connected TS server may be fetched.
use anyhow::{Result, bail};
use std::{net::SocketAddr, time::Duration};

fn banner_url(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value)?;
    if value.len() > 2048
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        bail!("服务器横幅仅支持直接访问的公网 HTTPS 图片（443端口）");
    }
    Ok(url)
}
pub async fn download(value: &str, max: usize) -> Result<Vec<u8>> {
    let result = tokio::time::timeout(Duration::from_secs(12), async {
        let url = banner_url(value)?;
        let host = url.host_str().unwrap().trim_matches(['[', ']']);
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, 443))
            .await?
            .take(17)
            .collect();
        if addresses.is_empty()
            || addresses.len() > 16
            || addresses
                .iter()
                .any(|a| !tsclientlib::resolver::is_public_addr(&a.ip()))
        {
            bail!("横幅地址不属于公网");
        }
        // Pin the validated DNS result while preserving hostname certificate verification.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(host, &addresses)
            .timeout(Duration::from_secs(10))
            .build()?;
        let mut response = client.get(url).send().await?;
        if !response.status().is_success() {
            bail!("横幅请求被拒绝或需要跳转，请管理员填写直接图片地址");
        }
        if response.content_length().is_some_and(|n| n > max as u64) {
            bail!("横幅超过站点图片限制");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len().saturating_add(chunk.len()) > max {
                bail!("横幅超过站点图片限制");
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    })
    .await;
    // Never expose upstream URLs (possibly containing tokens) or response bodies.
    result
        .unwrap_or_else(|_| Err(anyhow::anyhow!("横幅读取超时")))
        .map_err(|_| {
            anyhow::anyhow!("横幅读取失败：请使用直接公网 HTTPS 图片，并检查尺寸和站点限制")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn banner_urls_reject_credentials_protocols_ports_and_fragments() {
        assert!(banner_url("https://images.example.com/banner.png").is_ok());
        for value in [
            "http://example.com/a",
            "https://user:pass@example.com/a",
            "https://example.com:8443/a",
            "file:///a",
            "https://example.com/a#b",
        ] {
            assert!(banner_url(value).is_err(), "{value}");
        }
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
}
