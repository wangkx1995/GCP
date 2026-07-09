use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "agent.toml")]
    config: PathBuf,
}

#[derive(Deserialize)]
struct AgentConfig {
    core: CoreConfig,
    agent: AgentSettings,
}

#[derive(Deserialize)]
struct CoreConfig {
    host: String,
    port: u16,
    api_base: String,
    agent_id: String,
    reconnect_interval_ms: u64,
    reconnect_max_delay_ms: u64,
}

#[derive(Deserialize)]
struct AgentSettings {
    data_dir: PathBuf,
    #[allow(dead_code)]
    max_concurrent_tasks: u32,
    #[serde(default = "default_heartbeat_interval")]
    heartbeat_interval_seconds: u64,
}

fn default_heartbeat_interval() -> u64 { 10 }

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_content = std::fs::read_to_string(&cli.config)?;
    let config: AgentConfig = toml::from_str(&config_content)?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".parse().unwrap()),
        )
        .with_timer(wy_gnb_pm_parser::timeutil::East8Timer)
        .init();

    tracing::info!(
        "[agent] starting agent_id={} core_host={} core_port={} data_dir={}",
        config.core.agent_id,
        config.core.host,
        config.core.port,
        config.agent.data_dir.display()
    );

    wy_gnb_pm_parser::agent::server::run_agent_server(
        config.core.agent_id,
        config.core.host,
        config.core.port,
        config.core.api_base,
        config.agent.data_dir,
        None,
        config.core.reconnect_interval_ms,
        config.core.reconnect_max_delay_ms,
        config.agent.heartbeat_interval_seconds,
    )
    .await
}
