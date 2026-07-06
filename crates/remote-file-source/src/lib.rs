//! Resolves parser input files from either local paths or configured FTP/SFTP sources.

pub mod config;
mod ftp;
mod local;
mod remote;
mod sftp;
mod template;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use regex::Regex;
use tracing::{info, warn};

use config::SourceConfig;

/// Input resolution options for local files or configured FTP/SFTP sources.
pub struct ResolveOptions {
    /// Local file or directory input. Mutually exclusive with `source_config`.
    pub local_input: Option<PathBuf>,
    /// Recursively collect local input directories when `local_input` is a directory.
    pub recursive: bool,
    /// Parsed `SourceConfig` for FTP/SFTP input. Mutually exclusive with `local_input`.
    pub source_config: Option<config::SourceConfig>,
    /// Scan start time in `yyyy-MM-dd HH:mm:ss`; required for `source_config` mode.
    pub scan_start_time: Option<String>,
}

/// Files resolved for parsing plus optional route-based groups.
pub struct RoutedInputs {
    /// One representative local path per matched input, preserving legacy parser behavior.
    pub representative_files: Vec<PathBuf>,
    /// Downloaded files grouped by caller-provided route, such as destination table name.
    pub groups: Vec<RoutedInputGroup>,
}

/// Local files for one caller-provided route.
pub struct RoutedInputGroup {
    pub route: String,
    pub files: Vec<PathBuf>,
}

/// Resolves input files from a local path or downloads matching FTP/SFTP files first.
pub fn resolve_files(options: ResolveOptions) -> Result<Vec<PathBuf>> {
    resolve_files_with_router(options, |_| Vec::new())
}

/// Resolves input files and routes remote downloads into caller-provided subdirectories.
pub fn resolve_files_with_router<F>(
    options: ResolveOptions,
    route_remote_file: F,
) -> Result<Vec<PathBuf>>
where
    F: Fn(&str) -> Vec<String>,
{
    Ok(resolve_routed_files_with_router(options, route_remote_file)?.representative_files)
}

/// Resolves input files and returns both representative files and routed groups.
pub fn resolve_routed_files_with_router<F>(
    options: ResolveOptions,
    route_remote_file: F,
) -> Result<RoutedInputs>
where
    F: Fn(&str) -> Vec<String>,
{
    match (&options.local_input, &options.source_config) {
        (Some(_), Some(_)) => bail!("--input and --source-config cannot be used together"),
        (None, None) => bail!("either --input or --source-config is required"),
        (Some(input), None) => Ok(RoutedInputs {
            representative_files: local::collect_inputs(input, options.recursive)?,
            groups: Vec::new(),
        }),
        (None, Some(config)) => {
            let scan_start_time = options
                .scan_start_time
                .as_deref()
                .context("--scan-start-time is required when --source-config is used")?;
            resolve_remote_files(config, scan_start_time, &route_remote_file)
        }
    }
}

fn resolve_remote_files<F>(
    config: &SourceConfig,
    scan_start_time: &str,
    route_remote_file: &F,
) -> Result<RoutedInputs>
where
    F: Fn(&str) -> Vec<String>,
{
    let patterns = remote_patterns(&config.source.remote_pattern)?;
    let client = remote::connect_with_retry(config)?;
    let mut matched = Vec::new();
    let mut summaries = Vec::with_capacity(patterns.len());

    for (index, pattern) in patterns.iter().enumerate() {
        let scan_index = index + 1;
        let rendered = template::render_scan_start_time(pattern, scan_start_time)
            .with_context(|| format!("failed to render source.remote_pattern: {pattern}"))?;
        let scan_dir = template::infer_scan_dir(&rendered);
        let matcher = Regex::new(&rendered)
            .with_context(|| format!("invalid source.remote_pattern regex: {rendered}"))?;
        info!(
            "[source] remote context: index={}/{} type={:?} pattern={} rendered={} scan_dir={}",
            scan_index,
            patterns.len(),
            config.source.kind,
            pattern,
            rendered,
            scan_dir
        );

        match remote::list_files(&client, &scan_dir) {
            Ok(files) => {
                let scanned_count = files.len();
                let mut pattern_matched: Vec<String> = files
                    .into_iter()
                    .filter(|path| matcher.is_match(path))
                    .collect();
                let matched_count = pattern_matched.len();
                info!(
                    "[source] remote scan completed: index={}/{} scan_dir={} scanned={} matched={}",
                    scan_index,
                    patterns.len(),
                    scan_dir,
                    scanned_count,
                    matched_count
                );
                matched.append(&mut pattern_matched);
                summaries.push(ScanSummary {
                    index: scan_index,
                    scan_dir,
                    rendered,
                    scanned_count,
                    matched_count,
                    error: None,
                });
            }
            Err(err) => {
                warn!(
                    "[source] remote scan failed, skipping: index={}/{} scan_dir={} error={:#}",
                    scan_index,
                    patterns.len(),
                    scan_dir,
                    err
                );
                summaries.push(ScanSummary {
                    index: scan_index,
                    scan_dir,
                    rendered,
                    scanned_count: 0,
                    matched_count: 0,
                    error: Some(format!("{err:#}")),
                });
            }
        }
    }

    let successful_scans = summaries
        .iter()
        .filter(|summary| summary.error.is_none())
        .count();
    if successful_scans == 0 {
        bail!(
            "all remote scan directories failed: patterns={} scan_start_time={} details={}",
            patterns.len(),
            scan_start_time,
            format_scan_summaries(&summaries)
        );
    }

    matched.sort();
    matched.dedup();
    if matched.is_empty() {
        bail!(
            "remote scan completed but no file matched: patterns={} successful_scans={} failed_scans={} scan_start_time={} details={}",
            patterns.len(),
            successful_scans,
            summaries.len() - successful_scans,
            scan_start_time,
            format_scan_summaries(&summaries)
        );
    }
    info!(
        "[source] matched {} remote file(s) from {} successful scan(s), {} failed scan(s)",
        matched.len(),
        successful_scans,
        summaries.len() - successful_scans
    );

    remote::download_files_with_router(&client, config, &matched, route_remote_file)
}

fn remote_patterns(pattern: &str) -> Result<Vec<String>> {
    let patterns: Vec<String> = pattern
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if patterns.is_empty() {
        bail!("source.remote_pattern must not be empty");
    }
    Ok(patterns)
}

struct ScanSummary {
    index: usize,
    scan_dir: String,
    rendered: String,
    scanned_count: usize,
    matched_count: usize,
    error: Option<String>,
}

fn format_scan_summaries(summaries: &[ScanSummary]) -> String {
    summaries
        .iter()
        .map(|summary| {
            format!(
                "index={} scan_dir={} scanned={} matched={} regex={} error={}",
                summary.index,
                summary.scan_dir,
                summary.scanned_count,
                summary.matched_count,
                summary.rendered,
                summary.error.as_deref().unwrap_or("-")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_remote_patterns() {
        assert_eq!(remote_patterns("a;b").unwrap(), vec!["a", "b"]);
        assert_eq!(remote_patterns(" a ; ; b ").unwrap(), vec!["a", "b"]);
        assert_eq!(remote_patterns("a").unwrap(), vec!["a"]);
    }

    #[test]
    fn rejects_empty_remote_patterns() {
        let err = remote_patterns(" ; ; ").unwrap_err();
        assert!(err.to_string().contains("source.remote_pattern"));
    }
}
