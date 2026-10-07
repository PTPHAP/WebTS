use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub bind: String,
    pub public_url: String,
    pub database: String,
    pub master_key_file: String,
    pub web_dir: String,
    pub max_connections: usize,
    #[serde(default)]
    pub allow_insecure_localhost: bool,
    #[serde(default)]
    pub servers: Vec<Server>,
    pub smtp: Option<Smtp>,
    #[serde(default)]
    pub rtc: Rtc,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn load(text: &str) -> Result<Config> {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(text.as_bytes())?;
        Config::load(file.path().to_str().unwrap())
    }
    #[test]
    fn configuration_rejects_remote_plaintext_and_ambiguous_targets() {
        let example = include_str!("../../config.example.toml");
        assert!(load(example).is_ok());
        assert!(
            load(&example.replace("http://localhost:8080", "http://example.com:8080")).is_err()
        );
        assert!(load(&example.replace("127.0.0.1:8080", "0.0.0.0:8080")).is_err());
        assert!(
            load(&example.replace("http://localhost:8080", "https://user:password@example.com"))
                .is_err()
        );
        let duplicate = format!(
            "{example}\n[[servers]]\nid='one'\nname='a'\naddress='localhost:9987'\n[[servers]]\nid='one'\nname='b'\naddress='localhost:9988'\n"
        );
        assert!(load(&duplicate).is_err());
    }
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub address: String,
}
#[derive(Clone, Deserialize)]
pub struct Smtp {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password_file: String,
    pub from: String,
}
#[derive(Clone, Deserialize, Default)]
pub struct Rtc {
    #[serde(default)]
    pub public_ip: String,
    #[serde(default)]
    pub udp_min: u16,
    #[serde(default)]
    pub udp_max: u16,
    pub turn_url: Option<String>,
    pub turn_secret_file: Option<String>,
}
impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path).context("无法读取配置文件")?;
        let config: Self = toml::from_str(&text).context("配置格式错误")?;
        let url = url::Url::parse(&config.public_url).context("网站URL无效")?;
        let bind: std::net::SocketAddr = config.bind.parse().context("监听地址必须为IP:端口")?;
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if url.scheme() != "https"
            && !(url.scheme() == "http" && local && config.allow_insecure_localhost)
        {
            bail!("正式网站必须使用 HTTPS，HTTP 仅允许明确启用的 localhost 开发");
        }
        if url.scheme() == "http" && !bind.ip().is_loopback() {
            bail!("HTTP开发服务只能监听回环地址");
        }
        if url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            bail!("网站URL必须为无凭据、无路径的站点地址");
        }
        if config.max_connections == 0 || config.max_connections > 1000 {
            bail!("连接数限制无效");
        }
        let mut ids = std::collections::HashSet::new();
        for target in &config.servers {
            if target.id.is_empty() || !ids.insert(&target.id) || target.address.is_empty() {
                bail!("服务器ID必须唯一且地址不能为空");
            }
        }
        if config.rtc.turn_url.is_some() != config.rtc.turn_secret_file.is_some() {
            bail!("TURN地址与认证密钥必须同时配置");
        }
        Ok(config)
    }
    pub fn origin(&self) -> String {
        self.public_url.trim_end_matches('/').to_string()
    }
    pub fn secure_cookie(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}
