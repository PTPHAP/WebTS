use crate::{
    app::{Api, App, Error},
    config::Server,
    db::Session,
};
use anyhow::{Result, bail};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use lettre::{AsyncSmtpTransport, Tokio1Executor, transport::smtp::authentication::Credentials};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub home: crate::site::Home,
    pub servers: Vec<Server>,
    pub default_server: String,
    pub allow_custom: bool,
    pub smtp: Option<SmtpSettings>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub from: String,
    #[serde(default)]
    pub password: String,
}
impl Drop for SmtpSettings {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}
#[derive(Clone)]
pub struct Mailer {
    pub transport: AsyncSmtpTransport<Tokio1Executor>,
    pub from: lettre::message::Mailbox,
}
pub struct Runtime {
    pub settings: Settings,
    pub mailer: Option<Mailer>,
}
impl Runtime {
    pub fn new(settings: Settings) -> Result<Self> {
        settings.validate()?;
        let mailer = settings
            .smtp
            .as_ref()
            .map(|smtp| -> Result<Mailer> {
                if smtp.password.is_empty()
                    || smtp.password.len() > 1024
                    || smtp.port == 0
                    || smtp.host.len() > 253
                    || smtp.username.len() > 254
                {
                    bail!("邮箱配置无效");
                }
                let transport = if smtp.port == 465 {
                    AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)?
                } else {
                    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)?
                }
                .port(smtp.port)
                .timeout(Some(Duration::from_secs(15)))
                .credentials(Credentials::new(
                    smtp.username.clone(),
                    smtp.password.clone(),
                ))
                .build();
                Ok(Mailer {
                    transport,
                    from: smtp.from.parse()?,
                })
            })
            .transpose()?;
        Ok(Self { settings, mailer })
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        if self.servers.len() > 32 {
            bail!("最多32个服务器");
        }
        for server in &self.servers {
            if server.id.is_empty()
                || server.id.len() > 64
                || !server
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || !ids.insert(&server.id)
                || server.name.trim().is_empty()
                || server.name.chars().count() > 80
                || server.name.chars().any(char::is_control)
            {
                bail!("服务器配置无效");
            }
            normalize_target(&server.address)?;
        }
        if !self.default_server.is_empty() && !ids.contains(&self.default_server) {
            bail!("默认服务器不存在");
        }
        Ok(())
    }
    pub fn public(&self) -> Value {
        json!({"servers":self.servers.iter().map(|s|json!({"id":s.id,"name":s.name})).collect::<Vec<_>>(),"default_server":self.default_server,"allow_custom":self.allow_custom})
    }
    pub fn admin_view(&self) -> Value {
        json!({"servers":self.servers,"default_server":self.default_server,"allow_custom":self.allow_custom,"smtp":self.smtp.as_ref().map(|s|json!({"host":s.host,"port":s.port,"username":s.username,"from":s.from,"password_set":!s.password.is_empty()}))})
    }
}
// Custom addresses always have an explicit port: no nickname HTTP or TSDNS probing.
pub fn normalize_target(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 253
        || value.chars().any(char::is_whitespace)
        || value.contains(['/', '@', '?', '#', '\\'])
    {
        bail!("请输入公网主机和UDP端口");
    }
    let url = url::Url::parse(&format!("ts3://{value}"))?;
    let host = url::Host::parse(
        url.host_str()
            .ok_or_else(|| anyhow::anyhow!("地址缺少主机"))?,
    )?;
    match host {
        url::Host::Ipv4(ip) if !tsclientlib::resolver::is_public_addr(&ip.into()) => {
            bail!("不允许内网地址")
        }
        url::Host::Ipv6(ip) if !tsclientlib::resolver::is_public_addr(&ip.into()) => {
            bail!("不允许内网地址")
        }
        _ => {}
    }
    let port = url.port().unwrap_or(9987);
    if port == 0 {
        bail!("UDP端口无效");
    }
    Ok(format!("{host}:{port}"))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    password: String,
    settings: Settings,
}
pub(crate) fn admin(app: &App, headers: &HeaderMap) -> Api<Session> {
    let session = app.session(headers)?;
    if !session.user.is_admin {
        return Err(Error(StatusCode::FORBIDDEN, "仅网站管理员可以设置站点"));
    }
    Ok(session)
}
pub async fn get_settings(
    State(app): State<std::sync::Arc<App>>,
    headers: HeaderMap,
) -> Api<Json<Value>> {
    admin(&app, &headers)?;
    Ok(Json(app.runtime.read().unwrap().settings.admin_view()))
}
pub async fn save_settings(
    State(app): State<std::sync::Arc<App>>,
    headers: HeaderMap,
    Json(mut body): Json<Update>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        admin(a, &headers)?;
        let password = Zeroizing::new(body.password);
        let session = a.reauthenticate(&headers, &password)?;
        let mut current = a.runtime.write().unwrap();
        if let Some(smtp) = body.settings.smtp.as_mut()
            && smtp.password.is_empty()
            && let Some(old) = current.settings.smtp.as_ref()
        {
            if smtp.host != old.host || smtp.username != old.username {
                return Err(Error::bad("更换SMTP主机或登录账号时，请填写新的授权码"));
            }
            smtp.password = old.password.clone();
        }
        body.settings.home = current.settings.home.clone();
        let updated = Runtime::new(body.settings)
            .map_err(|_| Error::bad("配置无效，请检查服务器、默认项和邮箱参数"))?;
        let plaintext =
            Zeroizing::new(serde_json::to_vec(&updated.settings).map_err(anyhow::Error::from)?);
        let ciphertext = a.vault.seal(0, "site-settings", "v1", &plaintext)?;
        a.db.save_settings(session.user.id, &session.hash, &ciphertext)
            .map_err(|_| Error(StatusCode::FORBIDDEN, "管理员登录已失效，请重新登录"))?;
        a.connections
            .cancel_disallowed(&current.settings, &updated.settings);
        *current = updated;
        Ok(Json(
            json!({"message":"设置已保存并立即生效；正在发送的邮件会使用原配置完成。"}),
        ))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_targets_reject_local_and_metadata_addresses() {
        for address in [
            "127.0.0.1:9987",
            "10.1.2.3",
            "169.254.169.254:80",
            "[::1]:9987",
            "[::ffff:127.0.0.1]:9987",
            "[fc00::1]:9987",
            "100.64.1.1",
            "192.168.1.1",
            "ts.example.com:0",
            "user@ts.example.com",
            "https://example.com",
            "example.com/path",
        ] {
            assert!(normalize_target(address).is_err(), "{address}");
        }
        assert_eq!(
            normalize_target("ts.example.com").unwrap(),
            "ts.example.com:9987"
        );
        assert_eq!(normalize_target("1.1.1.1:9988").unwrap(), "1.1.1.1:9988");
        assert_eq!(
            normalize_target("[2606:4700:4700::1111]:9987").unwrap(),
            "[2606:4700:4700::1111]:9987"
        );
    }
}
