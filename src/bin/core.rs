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
    #[arg(long, default_value = "config_storage")]
    config_storage: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".parse().unwrap())
                .add_directive("sqlx=info".parse().unwrap()),
        )
        .init();
    let cli = Cli::parse();
    let config_storage =
        wy_gnb_pm_parser::core::config_storage::ConfigStorage::new(cli.config_storage)?;
    tracing::info!(
        "[core] starting listen={} db={} config_storage={:?}",
        cli.listen,
        cli.db.display(),
        config_storage.versions_dir()
    );
    let result =
        wy_gnb_pm_parser::core::server::run_core_server(cli.listen, cli.db, config_storage).await;
    result
}
