use std::path::{Path, PathBuf};
use std::io::Write;
use std::fs::OpenOptions;

use anyhow::Result;
use clap::Parser;
use serde::Deserialize;

use wy_gnb_pm_parser::agent::transfer::config::TransferConfig;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "agent.toml")]
    config: PathBuf,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Deserialize)]
struct AgentConfig {
    core: CoreConfig,
    agent: AgentSettings,
    #[serde(default)]
    transfer: TransferConfig,
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

fn default_heartbeat_interval() -> u64 {
    10
}

struct InstanceGuard {
    lock_path: PathBuf,
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        std::fs::remove_file(&self.lock_path).ok();
    }
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let ret = unsafe { libc::kill(pid as i32, 0) };
        if ret != 0 {
            return false;
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(cmdline) = std::fs::read_to_string(format!("/proc/{pid}/cmdline")) {
            if !cmdline.contains("agent") {
                return false;
            }
        }
    }

    true
}

fn check_single_instance(data_dir: &Path) -> Result<InstanceGuard> {
    let lock_path = data_dir.join("agent.lock");

    if let Ok(file) = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        write!(&file, "{}", std::process::id())?;
        file.sync_all()?;
        return Ok(InstanceGuard { lock_path });
    }

    let stale = match std::fs::read_to_string(&lock_path) {
        Ok(content) => match content.trim().parse::<u32>() {
            Ok(pid) => !pid_alive(pid),
            Err(_) => true,
        },
        Err(_) => true,
    };

    if stale {
        std::fs::remove_file(&lock_path).ok();
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|_| {
                anyhow::anyhow!(
                    "Agent lock conflict in {}. If stale, delete {} manually",
                    data_dir.display(),
                    lock_path.display()
                )
            })?;
        write!(&file, "{}", std::process::id())?;
        file.sync_all()?;
        return Ok(InstanceGuard { lock_path });
    }

    if let Ok(content) = std::fs::read_to_string(&lock_path) {
        let pid_hint = content.trim();
        if !pid_hint.is_empty() {
            anyhow::bail!(
                "Agent already running (pid={pid_hint}) in {}",
                data_dir.display()
            );
        }
    }
    anyhow::bail!("Agent already running in {}", data_dir.display())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_content = std::fs::read_to_string(&cli.config)?;
    let config: AgentConfig = toml::from_str(&config_content)?;
    config.transfer.validate()?;

    let _guard = check_single_instance(&config.agent.data_dir)?;

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
        config.transfer,
        cli.dry_run,
    )
    .await
}
