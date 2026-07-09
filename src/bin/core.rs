use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "server.toml")]
    config: PathBuf,
    #[arg(short = 's', long, default_value = "config_storage")]
    config_storage: PathBuf,
}

#[derive(Deserialize)]
struct ServerConfig {
    http: HttpConfig,
    tcp: TcpConfig,
    heartbeat: HeartbeatConfig,
    database: DatabaseConfig,
}

#[derive(Deserialize)]
struct HttpConfig {
    host: String,
    port: u16,
}

#[derive(Deserialize)]
struct TcpConfig {
    bind_host: String,
    bind_port: u16,
}

#[derive(Deserialize)]
struct HeartbeatConfig {
    #[serde(default = "default_cleanup_interval")]
    cleanup_interval_seconds: u64,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

fn default_cleanup_interval() -> u64 { 30 }
fn default_timeout_ms() -> u64 { 150_000 }

#[derive(Deserialize)]
struct DatabaseConfig {
    url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_content = std::fs::read_to_string(&cli.config)?;
    let config: ServerConfig = toml::from_str(&config_content)?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".parse().unwrap())
                .add_directive("sqlx=info".parse().unwrap()),
        )
        .with_timer(wy_gnb_pm_parser::timeutil::East8Timer)
        .init();

    let http_addr: SocketAddr = format!("{}:{}", config.http.host, config.http.port).parse()?;
    let tcp_addr: SocketAddr =
        format!("{}:{}", config.tcp.bind_host, config.tcp.bind_port).parse()?;
    let db_path = PathBuf::from(&config.database.url);
    let config_storage =
        wy_gnb_pm_parser::core::config_storage::ConfigStorage::new(cli.config_storage)?;

    tracing::info!(
        "[core] starting http={} tcp={} db={} config_storage={:?}",
        http_addr,
        tcp_addr,
        db_path.display(),
        config_storage.versions_dir()
    );

    wy_gnb_pm_parser::core::server::run_core_server(
        http_addr, tcp_addr, db_path, config_storage,
        config.heartbeat.cleanup_interval_seconds,
        config.heartbeat.timeout_ms,
    ).await
}
