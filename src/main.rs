use hancic::config::Config;
use std::net::SocketAddr;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,hancic=debug".into()),
    ).init();

    let config_path = std::env::args().nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/config.toml"));
    let config = Config::load(&config_path).map_err(anyhow::Error::msg)?;
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let app = hancic::app(config)
        .await
        .map_err(|e| anyhow::anyhow!("应用启动失败: {}", e.message()))?;
    tracing::info!("hancic listening on {addr}");
    // 必须带 ConnectInfo：/admin/login 按 IP 限流依赖该扩展（与 tests/common 一致）
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;
    Ok(())
}
