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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    wy_gnb_pm_parser::agent::server::run_agent_server(cli.listen, cli.data_dir, cli.core_api_base, cli.agent_id).await
}
