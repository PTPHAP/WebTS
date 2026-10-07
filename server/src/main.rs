use anyhow::{Context, Result, bail};
use std::{fs::OpenOptions, io::Write};
use web_ts::{identity, probe};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("web_ts=info,tsclientlib=error,tsproto=error")
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") if args.len() == 2 => {
            let config = web_ts::config::Config::load(&args[1])?;
            let bind = config.bind.clone();
            let app = web_ts::app::App::new(config)?;
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            tracing::info!(address = %bind, "WebTS 服务已启动");
            let shutdown = app.clone();
            axum::serve(
                listener,
                web_ts::app::router(app)
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                shutdown_signal().await;
                shutdown.connections.shutdown();
            })
            .await?;
        }
        Some("init-key") if args.len() == 2 => {
            let mut key = zeroize::Zeroizing::new([0u8; 32]);
            getrandom::fill(key.as_mut()).map_err(|_| anyhow::anyhow!("随机源不可用"))?;
            let encoded = zeroize::Zeroizing::new(hex::encode(*key));
            write_new(&args[1], encoded.as_bytes())?;
            println!("部署密钥已创建；请限制文件权限并单独备份。");
        }
        Some("create-identity") if args.len() == 2 => {
            let value = tsclientlib::Identity::create();
            write_new(&args[1], identity::export(&value, "WebTS").as_bytes())?;
            println!("身份已创建，UID：{}", identity::uid(&value));
        }
        Some("probe") if args.len() == 4 || args.len() == 5 => {
            let text = zeroize::Zeroizing::new(
                std::fs::read_to_string(&args[2]).context("无法读取身份文件")?,
            );
            let value = identity::parse(&text)?;
            let seconds = args
                .get(4)
                .map(|s| s.parse::<u64>())
                .transpose()?
                .unwrap_or(10);
            let report = probe::run(args[1].clone(), value, seconds).await?;
            write_new(&args[3], &serde_json::to_vec_pretty(&report)?)?;
            println!("协议探针报告已保存。此报告不代表原生客户端双向语音已验收。");
        }
        _ => bail!(
            "用法：web-ts serve <配置.toml> | init-key <密钥路径> | create-identity <身份.ini> | probe <地址:UDP端口> <身份.ini> <报告.json> [秒数]"
        ),
    }
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

fn write_new(path: &str, data: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .context("无法创建文件（不会覆盖已有密钥、身份或报告）")?;
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}
