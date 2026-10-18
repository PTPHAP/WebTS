//! Local, explicit reconfiguration. Never accepts a TS target from a pairing response.
use super::*;
use std::{
    fs::OpenOptions,
    io::{Read, Seek, Write},
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    main_url: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    public_ip: Option<String>,
    #[serde(default)]
    code: String,
    #[serde(default)]
    ts_port: Option<u16>,
    #[serde(default)]
    file_transfer_port: Option<u16>,
    #[serde(default)]
    udp_min: Option<u16>,
    #[serde(default)]
    udp_max: Option<u16>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    id: String,
    main_url: String,
    key: String,
}
impl Drop for Reply {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

/// Operational fields only: never print pairing keys or TURN credentials.
pub fn inspect(path: &str) -> Result<()> {
    let config = NodeConfig::load(path)?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "id":config.id, "bind":config.bind, "main_url":config.main_url,
            "target":config.target, "file_transfer_port":config.file_transfer_port,
            "key_file":config.key_file, "public_ip":config.rtc.public_ip,
            "udp_min":config.rtc.udp_min, "udp_max":config.rtc.udp_max,
            "max_connections":config.max_connections,
        }))?
    );
    Ok(())
}

pub async fn configure(path: &str) -> Result<()> {
    let mut raw = Zeroizing::new(String::new());
    std::io::stdin().take(4097).read_to_string(&mut raw)?;
    if raw.len() > 4096 {
        bail!("配置输入过长");
    }
    configure_json(path, &raw).await
}

async fn configure_json(path: &str, raw: &str) -> Result<()> {
    let mut input: Input = serde_json::from_str(raw).context("配置输入必须是JSON")?;
    let code = Zeroizing::new(std::mem::take(&mut input.code));
    let path = Path::new(path);
    let old_text = Zeroizing::new(std::fs::read_to_string(path)?);
    let old: NodeConfig = toml::from_str(&old_text)?;
    old.validate()?;
    let main = endpoint(&input.main_url, old.allow_insecure_localhost)?;
    let id = input.id.as_deref().unwrap_or(&old.id);
    if !valid_id(id) {
        bail!("节点标识只允许字母、数字、短横线或下划线，最多64字节");
    }
    if code.is_empty()
        && (id != old.id
            || old.main_url.as_deref().is_none_or(|url| {
                endpoint(url, old.allow_insecure_localhost).ok().as_ref() != Some(&main)
            }))
    {
        bail!("首次配对、更换主站或节点标识时必须输入新的配对码");
    }
    let _reserved = tokio::net::TcpListener::bind(old.bind)
        .await
        .context("节点正在运行或端口被占用，请先停止节点再配置；尚未领取配对码")?;
    let mut document: toml::Value = toml::from_str(&old_text)?;
    let table = document.as_table_mut().context("节点配置无效")?;
    table.remove("public_url");
    table.insert("id".into(), toml::Value::String(id.into()));
    table.insert(
        "main_url".into(),
        toml::Value::String(main.as_str().trim_end_matches('/').into()),
    );
    if let Some(port) = input.ts_port {
        let mut target = old.target;
        target.set_port(port);
        table.insert("target".into(), toml::Value::String(target.to_string()));
    }
    if let Some(port) = input.file_transfer_port {
        table.insert(
            "file_transfer_port".into(),
            toml::Value::Integer(port.into()),
        );
    }
    match (input.udp_min, input.udp_max) {
        (Some(min), Some(max)) => {
            let rtc = table
                .get_mut("rtc")
                .and_then(toml::Value::as_table_mut)
                .context("RTC配置无效")?;
            rtc.insert("udp_min".into(), toml::Value::Integer(min.into()));
            rtc.insert("udp_max".into(), toml::Value::Integer(max.into()));
        }
        (None, None) => {}
        _ => bail!("语音UDP起止端口必须同时填写"),
    }
    if let Some(ip) = input.public_ip {
        if !ip.is_empty() {
            let ip: std::net::Ipv4Addr = ip.parse().context("公网IP必须为IPv4地址，不填域名")?;
            if ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_private()
                || ip.is_link_local()
            {
                bail!("请填写真实公网IPv4，不能填回环或内网IP");
            }
        }
        table
            .entry("rtc")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .context("RTC配置无效")?
            .insert("public_ip".into(), toml::Value::String(ip));
    }
    let text = toml::to_string_pretty(&document)?;
    let config: NodeConfig = toml::from_str(&text)?;
    config.validate()?;
    let old_key = Zeroizing::new(
        std::fs::read_to_string(&config.key_file)
            .context("请先运行init生成受保护的节点密钥文件")?,
    );
    key_from_text(&old_key)?;
    // Check local write permissions before consuming a one-time code. No truncation yet.
    let mut key_file = OpenOptions::new()
        .write(true)
        .open(&config.key_file)
        .context("当前账号不能更新配对密钥")?;
    let mut config_file = OpenOptions::new()
        .write(true)
        .open(path)
        .context("当前账号不能更新节点配置")?;
    let mut key_changed = false;
    let mut config_changed = false;
    let result = async {
        let new_key = if code.is_empty() {
            None
        } else {
            if code.len() != 64 || !code.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("配对码格式无效");
            }
            let _ = rustls::crypto::ring::default_provider().install_default();
            let response = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(8))
                .build()?
                .post(main.join("/api/voice-nodes/pair")?)
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&json!({"code":code.as_str()}))?)
                .send()
                .await?;
            if !response.status().is_success() {
                bail!("主站拒绝配对：检查配对码期限、管理员登录和主站版本；未修改本地密钥");
            }
            let mut stream = response.bytes_stream();
            let mut bytes = Zeroizing::new(Vec::new());
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                if bytes.len() + chunk.len() > 2048 {
                    bail!("配对响应过长");
                }
                bytes.extend_from_slice(&chunk);
            }
            let reply: Reply = serde_json::from_slice(&bytes)?;
            if reply.id != config.id
                || endpoint(&reply.main_url, config.allow_insecure_localhost)? != main
            {
                bail!("配对节点ID或主站地址不符，未修改本地密钥；请重新生成正确配对码");
            }
            key_from_text(&reply.key)?;
            Some(Zeroizing::new(reply.key.clone()))
        };
        if let Some(key) = &new_key {
            key_changed = true;
            replace(&mut key_file, key.as_bytes())?;
        }
        // Update existing files to retain explicit Windows ACLs as well as Unix
        // ownership/mode. The stopped-node reservation excludes another wizard.
        config_changed = true;
        replace(&mut config_file, text.as_bytes())?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if result.is_err() {
        let restore_config = if config_changed {
            replace(&mut config_file, old_text.as_bytes())
        } else {
            Ok(())
        };
        let restore_key = if key_changed {
            replace(&mut key_file, old_key.as_bytes())
        } else {
            Ok(())
        };
        restore_config.context("配置恢复失败，请停止节点并从安全备份恢复")?;
        restore_key.context("密钥恢复失败，请停止节点并重新配对")?;
    }
    result?;
    println!(
        "反向节点配置已保存，TS目标仍仅限本机回环；TS机器无需公开节点HTTPS入口。请启动节点并查看主站通道状态。实际语音、网络和部署要求仍需验收。"
    );
    Ok(())
}

fn replace(file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    file.rewind()?;
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[tokio::test]
    async fn configure_keeps_fixed_ts_and_refuses_busy_invalid_or_wrong_pairing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.local.toml");
        let key = directory.path().join("pair.key");
        let original_key = hex::encode([71; 32]);
        std::fs::write(&key, &original_key).unwrap();
        let local = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config: toml::Value =
            toml::from_str(include_str!("../../../node.example.toml")).unwrap();
        config["bind"] = toml::Value::String(local.local_addr().unwrap().to_string());
        config["key_file"] = toml::Value::String(key.to_string_lossy().into());
        config["allow_insecure_localhost"] = toml::Value::Boolean(true);
        let original = toml::to_string(&config).unwrap();
        std::fs::write(&path, &original).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let main = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let wrong = Arc::new(AtomicBool::new(false));
        let (count, bad, url) = (calls.clone(), wrong.clone(), main.clone());
        let route = Router::new().route("/api/voice-nodes/pair", axum::routing::post(move |Json(body):Json<Value>| {
            let (count,bad,url)=(count.clone(),bad.clone(),url.clone());
            async move {
                assert_eq!(body["code"], "a".repeat(64));
                count.fetch_add(1,Ordering::SeqCst);
                Json(json!({"id":if bad.load(Ordering::SeqCst){"wrong"}else{"community-node"},"main_url":url,"key":hex::encode([73;32])}))
            }
        }));
        let stop = CancellationToken::new();
        let signal = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, route)
                .with_graceful_shutdown(signal.cancelled_owned())
                .await
                .unwrap();
        });
        let body =
            json!({"main_url":main,"public_ip":"203.0.113.20","code":"a".repeat(64)}).to_string();
        let name = path.to_str().unwrap();
        assert!(configure_json(name, &body).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(local);
        for id in ["", "../wrong", "two nodes", &"x".repeat(65)] {
            assert!(
                configure_json(
                    name,
                    &json!({"id":id,"main_url":main,"code":"a".repeat(64)}).to_string()
                )
                .await
                .is_err()
            );
        }
        assert!(
            configure_json(name, &json!({"id":"new-node","main_url":main}).to_string())
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(
            configure_json(
                name,
                &json!({"main_url":main,"public_ip":"127.0.0.1","code":"a".repeat(64)}).to_string()
            )
            .await
            .is_err()
        );
        assert!(
            configure_json(
                name,
                &json!({"main_url":main,"code":"malformed"}).to_string()
            )
            .await
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        for field in ["ts_port", "file_transfer_port"] {
            let mut invalid = json!({"main_url":main,"code":"a".repeat(64)});
            invalid[field] = json!(0);
            assert!(configure_json(name, &invalid.to_string()).await.is_err());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        for ports in [
            json!({"udp_min":51000}),
            json!({"udp_min":0,"udp_max":51099}),
            json!({"udp_min":51099,"udp_max":51000}),
            json!({"udp_min":51000,"udp_max":51001}),
        ] {
            let mut invalid = ports;
            invalid["main_url"] = json!(main);
            invalid["code"] = json!("a".repeat(64));
            assert!(configure_json(name, &invalid.to_string()).await.is_err());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        wrong.store(true, Ordering::SeqCst);
        assert!(configure_json(name, &body).await.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(std::fs::read_to_string(&key).unwrap(), original_key);
        wrong.store(false, Ordering::SeqCst);
        configure_json(name, &body).await.unwrap();
        let updated = NodeConfig::load(name).unwrap();
        assert_eq!(updated.target.to_string(), "127.0.0.1:9987");
        assert_eq!(updated.file_transfer_port, 30033);
        assert_eq!(updated.main_url.as_deref(), Some(main.as_str()));
        assert!(updated.public_url.is_empty());
        assert_eq!(updated.rtc.public_ip, "203.0.113.20");
        assert_eq!(
            std::fs::read_to_string(&key).unwrap(),
            hex::encode([73; 32])
        );
        configure_json(
            name,
            &json!({"main_url":main,"ts_port":19987,"file_transfer_port":30034,"udp_min":51000,"udp_max":51099}).to_string(),
        )
        .await
        .unwrap();
        let changed = NodeConfig::load(name).unwrap();
        assert_eq!(changed.target.to_string(), "127.0.0.1:19987");
        assert_eq!(changed.file_transfer_port, 30034);
        assert_eq!((changed.rtc.udp_min, changed.rtc.udp_max), (51000, 51099));
        assert_eq!(
            std::fs::read_to_string(&key).unwrap(),
            hex::encode([73; 32])
        );
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains(&"a".repeat(64))
        );
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".node-config-")
        }));
        stop.cancel();
        task.await.unwrap();
    }
}
