use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:18081")]
    listen: SocketAddr,
    #[arg(long, default_value = "agent_data")]
    data_dir: PathBuf,
    #[arg(long, default_value = "http://127.0.0.1:18080/api")]
    core_api_base: String,
    #[arg(long, default_value = "agent_local")]
    agent_id: String,
    #[arg(long)]
    config_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();
    let cli = Cli::parse();
    tracing::info!("[agent] starting agent_id={} listen={} data_dir={} core_api_base={} config_dir={:?}", cli.agent_id, cli.listen, cli.data_dir.display(), cli.core_api_base, cli.config_dir);
    let result = wy_gnb_pm_parser::agent::server::run_agent_server(cli.listen, cli.data_dir, cli.core_api_base, cli.agent_id, cli.config_dir).await;
    result
}
