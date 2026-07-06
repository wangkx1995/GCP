use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "agent_local")]
    agent_id: String,
    #[arg(long, default_value = "127.0.0.1")]
    core_host: String,
    #[arg(long, default_value = "18082")]
    core_port: u16,
    #[arg(long, default_value = "http://127.0.0.1:18080/api")]
    core_api_base: String,
    #[arg(long, default_value = "agent_data")]
    data_dir: PathBuf,
    #[arg(long, default_value = "5000")]
    reconnect_interval_ms: u64,
    #[arg(long, default_value = "60000")]
    reconnect_max_delay_ms: u64,
    #[arg(long)]
    config_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".parse().unwrap()),
        )
        .init();
    let cli = Cli::parse();
    tracing::info!(
        "[agent] starting agent_id={} core_host={} core_port={} data_dir={} config_dir={:?}",
        cli.agent_id, cli.core_host, cli.core_port, cli.data_dir.display(), cli.config_dir
    );
    let result = wy_gnb_pm_parser::agent::server::run_agent_server(
        cli.agent_id,
        cli.core_host,
        cli.core_port,
        cli.core_api_base,
        cli.data_dir,
        cli.config_dir,
        cli.reconnect_interval_ms,
        cli.reconnect_max_delay_ms,
    )
    .await;
    result
}
