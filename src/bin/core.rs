use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:18080")]
    listen: SocketAddr,
    #[arg(long, default_value = "core.db")]
    db: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();
    let cli = Cli::parse();
    tracing::info!("[core] starting listen={} db={}", cli.listen, cli.db.display());
    let result = wy_gnb_pm_parser::core::server::run_core_server(cli.listen, cli.db).await;
    result
}
