use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use wy_gnb_pm_parser::parse_job::{cleanup_old_logs, run_parse_job, ParseJobOptions};
use wy_gnb_pm_parser::LoadType;

#[derive(Parser)]
#[command(name = "wy-gnb-pm-parser")]
#[command(about = "Parse WY GNB PM files into per-table UTF-8 CSV files")]
struct Cli {
    #[arg(long)]
    input: Option<PathBuf>,
    #[arg(long)]
    source_config: Option<PathBuf>,
    #[arg(long)]
    scan_start_time: Option<String>,
    #[arg(long, default_value = ".")]
    config_dir: PathBuf,
    #[arg(long)]
    output_dir: PathBuf,
    #[arg(long, value_enum)]
    load_type: LoadType,
    #[arg(long, default_value = "load.toml")]
    load_config: PathBuf,
    #[arg(long, default_value = "|")]
    output_delimiter: String,
    #[arg(long, default_value = "UTF-8")]
    encoding: String,
    #[arg(long)]
    recursive: bool,
    #[arg(long = "rule-file")]
    rule_files: Vec<PathBuf>,
    #[arg(long = "rules-dir")]
    rules_dir: Option<PathBuf>,
}

fn main() -> Result<()> {
    let start = Instant::now();

    let log_dir = Path::new("logs");
    fs::create_dir_all(log_dir)?;
    cleanup_old_logs(log_dir, 30)?;

    let file_appender = tracing_appender::rolling::daily(log_dir, "app.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into());

    use wy_gnb_pm_parser::timeutil::East8Timer;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_timer(East8Timer)
                .with_writer(std::io::stderr)
                .with_filter(filter.clone()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_timer(East8Timer)
                .with_writer(non_blocking)
                .with_filter(filter),
        )
        .init();

    let cli = Cli::parse();
    let source_config = match &cli.source_config {
        Some(path) => Some(remote_file_source::config::load_source_config(path)?),
        None => None,
    };
    let load_config = wy_gnb_pm_parser::load_config::load_config(&cli.load_config)?;
    let summary = run_parse_job(ParseJobOptions {
        input: cli.input,
        source_config,
        scan_start_time: cli.scan_start_time,
        config_dir: cli.config_dir,
        output_dir: cli.output_dir,
        collector_name: "cli".to_string(),
        load_type: cli.load_type,
        load_config,
        output_delimiter: cli.output_delimiter,
        encoding: cli.encoding,
        recursive: cli.recursive,
        rule_files: cli.rule_files,
        rules_dir: cli.rules_dir,
        log_file: None,
    })?;
    info!("[done] {} streaming destination table task(s)", summary.task_count);

    info!("[done] {:.2}s total", start.elapsed().as_secs_f64());
    Ok(())
}
