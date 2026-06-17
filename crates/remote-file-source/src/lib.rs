//! Resolves parser input files from either local paths or configured FTP/SFTP sources.

mod config;
mod ftp;
mod local;
mod remote;
mod sftp;
mod template;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use regex::Regex;

use config::SourceConfig;

/// Input resolution options for local files or configured FTP/SFTP sources.
pub struct ResolveOptions {
    /// Local file or directory input. Mutually exclusive with `source_config`.
    pub local_input: Option<PathBuf>,
    /// Recursively collect local input directories when `local_input` is a directory.
    pub recursive: bool,
    /// Path to `source.toml` for FTP/SFTP input. Mutually exclusive with `local_input`.
    pub source_config: Option<PathBuf>,
    /// Scan start time in `yyyy-MM-dd HH:mm:ss`; required for `source_config` mode.
    pub scan_start_time: Option<String>,
}

/// Resolves input files from a local path or downloads matching FTP/SFTP files first.
pub fn resolve_files(options: ResolveOptions) -> Result<Vec<PathBuf>> {
    match (&options.local_input, &options.source_config) {
        (Some(_), Some(_)) => bail!("--input and --source-config cannot be used together"),
        (None, None) => bail!("either --input or --source-config is required"),
        (Some(input), None) => local::collect_inputs(input, options.recursive),
        (None, Some(config_path)) => {
            let scan_start_time = options
                .scan_start_time
                .as_deref()
                .context("--scan-start-time is required when --source-config is used")?;
            let config = config::load_source_config(config_path)
                .with_context(|| format!("failed to parse {}", config_path.display()))?;
            resolve_remote_files(&config, scan_start_time)
        }
    }
}

fn resolve_remote_files(config: &SourceConfig, scan_start_time: &str) -> Result<Vec<PathBuf>> {
    let rendered =
        template::render_scan_start_time(&config.source.remote_pattern, scan_start_time)?;
    let scan_dir = template::infer_scan_dir(&rendered);
    let matcher = Regex::new(&rendered)
        .with_context(|| format!("invalid source.remote_pattern regex: {rendered}"))?;
    eprintln!(
        "[source] remote context: type={:?} pattern={} rendered={} scan_dir={}",
        config.source.kind, config.source.remote_pattern, rendered, scan_dir
    );

    let client = remote::connect_with_retry(config)?;
    let files = remote::list_files(&client, &scan_dir)
        .with_context(|| format!("failed to scan remote directory: {scan_dir}"))?;
    let scanned_count = files.len();
    let mut matched: Vec<String> = files
        .into_iter()
        .filter(|path| matcher.is_match(path))
        .collect();
    matched.sort();
    if matched.is_empty() {
        bail!(
            "remote scan completed but no file matched: type={:?} scan_dir={} scanned_files={} regex={} scan_start_time={}",
            config.source.kind,
            scan_dir,
            scanned_count,
            rendered,
            scan_start_time
        );
    }
    eprintln!("[source] matched {} remote file(s)", matched.len());

    remote::download_files(&client, config, &matched)
}
