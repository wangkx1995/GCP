use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

use crate::config::{SourceConfig, SourceKind};
use crate::{ftp, sftp};

pub(crate) enum RemoteClient {
    Ftp(ftp::FtpClient),
    Sftp(sftp::SftpClient),
}

pub(crate) fn connect_with_retry(config: &SourceConfig) -> Result<RemoteClient> {
    let source = &config.source;
    let conn = &source.connection;
    let attempts = source.connect_retry;
    let mut last_error = None;
    for attempt in 1..=attempts {
        let result = match source.kind {
            SourceKind::Ftp => ftp::connect(config).map(RemoteClient::Ftp),
            SourceKind::Sftp => sftp::connect(config).map(RemoteClient::Sftp),
        };
        match result {
            Ok(client) => {
                eprintln!(
                    "[source] connected: type={:?} host={} port={} user={}",
                    source.kind, conn.host, conn.port, conn.username
                );
                return Ok(client);
            }
            Err(err) => {
                eprintln!(
                    "[source] connect attempt {}/{} failed: type={:?} host={} port={} user={} error={:#}",
                    attempt, attempts, source.kind, conn.host, conn.port, conn.username, err
                );
                last_error = Some(err);
                wait_before_retry(attempt, attempts, source.retry_interval_secs, "connect");
            }
        }
    }
    Err(last_error.expect("connect attempts must be greater than 0"))
}

pub(crate) fn list_files(client: &RemoteClient, scan_dir: &str) -> Result<Vec<String>> {
    match client {
        RemoteClient::Ftp(client) => client.list_files(scan_dir),
        RemoteClient::Sftp(client) => client.list_files(scan_dir),
    }
}

pub(crate) fn download_files_with_router<F>(
    client: &RemoteClient,
    config: &SourceConfig,
    remote_files: &[String],
    route_remote_file: &F,
) -> Result<Vec<PathBuf>>
where
    F: Fn(&str) -> Vec<String>,
{
    fs::create_dir_all(&config.source.download_dir)?;
    cleanup_download_dir(config)?;
    let targets = download_targets(config, remote_files, route_remote_file);
    if config.source.download_parallel > 1 {
        return download_files_parallel(config, targets, remote_files.len());
    }
    download_files_sequential(client, config, targets, remote_files.len())
}

fn download_files_sequential(
    client: &RemoteClient,
    config: &SourceConfig,
    targets: Vec<DownloadTarget>,
    remote_file_count: usize,
) -> Result<Vec<PathBuf>> {
    let mut local_files = vec![None; remote_file_count];
    for target in targets {
        download_one_with_retry(client, config, &target.remote_file, &target.local_path)
            .with_context(|| format!("failed to download remote file {}", target.remote_file))?;
        if target.route_index == 0 {
            local_files[target.remote_index] = Some(target.local_path);
        }
    }
    representative_paths(local_files)
}

fn download_files_parallel(
    config: &SourceConfig,
    targets: Vec<DownloadTarget>,
    remote_file_count: usize,
) -> Result<Vec<PathBuf>> {
    let target_count = targets.len();
    let workers = config.source.download_parallel.min(target_count);
    eprintln!(
        "[source] downloading {} remote target(s) for {} remote file(s) with {} worker(s)",
        target_count, remote_file_count, workers
    );
    let queue = Arc::new(Mutex::new(targets.into_iter().collect::<VecDeque<_>>()));
    let successes = Arc::new(Mutex::new(Vec::new()));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::with_capacity(workers);

    for worker_id in 1..=workers {
        let queue = Arc::clone(&queue);
        let successes = Arc::clone(&successes);
        let failures = Arc::clone(&failures);
        let config = config.clone();
        handles.push(thread::spawn(move || {
            let client = match connect_with_retry(&config) {
                Ok(client) => client,
                Err(err) => {
                    eprintln!(
                        "[source] download worker {} failed to create connection: {err:#}",
                        worker_id
                    );
                    return;
                }
            };

            loop {
                let next = queue.lock().expect("queue mutex poisoned").pop_front();
                let Some(target) = next else {
                    break;
                };
                match download_one_with_retry(
                    &client,
                    &config,
                    &target.remote_file,
                    &target.local_path,
                ) {
                    Ok(()) => successes.lock().expect("successes mutex poisoned").push((
                        target.remote_index,
                        target.route_index,
                        target.local_path,
                    )),
                    Err(err) => failures.lock().expect("failures mutex poisoned").push((
                        target.remote_index,
                        target.route_index,
                        target.remote_file,
                        format!("{err:#}"),
                    )),
                }
            }
        }));
    }

    for handle in handles {
        handle.join().expect("download worker panicked");
    }

    let remaining = queue
        .lock()
        .expect("queue mutex poisoned")
        .drain(..)
        .collect::<Vec<_>>();
    if !remaining.is_empty() {
        let mut failures = failures.lock().expect("failures mutex poisoned");
        for target in remaining {
            failures.push((
                target.remote_index,
                target.route_index,
                target.remote_file,
                "no download worker was available".to_string(),
            ));
        }
    }

    let mut failures = failures.lock().expect("failures mutex poisoned");
    failures.sort_by_key(|(remote_index, route_index, _, _)| (*remote_index, *route_index));
    if !failures.is_empty() {
        let failure_count = failures.len();
        let success_count = successes.lock().expect("successes mutex poisoned").len();
        let details = failures
            .iter()
            .map(|(_, _, remote_file, err)| format!("remote={} error={}", remote_file, err))
            .collect::<Vec<_>>()
            .join("; ");
        bail!(
            "parallel download completed with failures: success={} failed={} details={}",
            success_count,
            failure_count,
            details
        );
    }

    let mut successes = successes.lock().expect("successes mutex poisoned");
    successes.sort_by_key(|(remote_index, route_index, _)| (*remote_index, *route_index));
    let mut local_files = vec![None; remote_file_count];
    for (remote_index, route_index, path) in successes.iter() {
        if *route_index == 0 {
            local_files[*remote_index] = Some(path.clone());
        }
    }
    representative_paths(local_files)
}

fn download_one_with_retry(
    client: &RemoteClient,
    config: &SourceConfig,
    remote_file: &str,
    local_path: &Path,
) -> Result<()> {
    if let Some(parent) = local_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let attempts = config.source.download_retry;
    let mut last_error = None;
    for attempt in 1..=attempts {
        let part_path = local_path.with_file_name(format!(
            "{}.part",
            local_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("download")
        ));
        let result = match client {
            RemoteClient::Ftp(client) => client.download_file(remote_file, &part_path),
            RemoteClient::Sftp(client) => client.download_file(remote_file, &part_path),
        }
        .and_then(|_| {
            if local_path.exists() {
                eprintln!(
                    "[source] local file exists, overwriting: {}",
                    local_path.display()
                );
                fs::remove_file(local_path)?;
            }
            fs::rename(&part_path, local_path)?;
            Ok(())
        });

        match result {
            Ok(()) => {
                eprintln!(
                    "[source] downloaded: {} -> {}",
                    remote_file,
                    local_path.display()
                );
                return Ok(());
            }
            Err(err) => {
                let _ = fs::remove_file(&part_path);
                eprintln!(
                    "[source] download attempt {}/{} failed: remote={} local={} error={:#}",
                    attempt,
                    attempts,
                    remote_file,
                    local_path.display(),
                    err
                );
                last_error = Some(err);
                wait_before_retry(
                    attempt,
                    attempts,
                    config.source.retry_interval_secs,
                    "download",
                );
            }
        }
    }
    Err(last_error.expect("download attempts must be greater than 0"))
}

fn wait_before_retry(attempt: usize, attempts: usize, interval_secs: u64, operation: &str) {
    if attempt < attempts {
        eprintln!(
            "[source] waiting {}s before next {} attempt",
            interval_secs, operation
        );
        thread::sleep(Duration::from_secs(interval_secs));
    }
}

fn cleanup_download_dir(config: &SourceConfig) -> Result<()> {
    let retention = Duration::from_secs(
        config
            .source
            .cache_retention_days
            .saturating_mul(24 * 60 * 60),
    );
    let now = SystemTime::now();
    for entry in WalkDir::new(&config.source.download_dir).min_depth(1) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                eprintln!("[source] cache cleanup skipped unreadable entry: {err:#}");
                continue;
            }
        };
        let path = entry.path();
        let file_type = entry.file_type();
        if !file_type.is_file() {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                eprintln!(
                    "[source] cache cleanup skipped {}: failed to read metadata: {err:#}",
                    path.display()
                );
                continue;
            }
        };
        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(err) => {
                eprintln!(
                    "[source] cache cleanup skipped {}: failed to read modified time: {err:#}",
                    path.display()
                );
                continue;
            }
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age < retention {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => eprintln!("[source] cache cleanup deleted: {}", path.display()),
            Err(err) => eprintln!(
                "[source] cache cleanup failed to delete {}: {err:#}",
                path.display()
            ),
        }
    }
    Ok(())
}

fn representative_paths(local_files: Vec<Option<PathBuf>>) -> Result<Vec<PathBuf>> {
    local_files
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            path.with_context(|| {
                format!("missing representative download for remote index {index}")
            })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DownloadTarget {
    remote_index: usize,
    route_index: usize,
    remote_file: String,
    local_path: PathBuf,
}

fn download_targets<F>(
    config: &SourceConfig,
    remote_files: &[String],
    route_remote_file: &F,
) -> Vec<DownloadTarget>
where
    F: Fn(&str) -> Vec<String>,
{
    remote_files
        .iter()
        .enumerate()
        .flat_map(|(remote_index, remote_file)| {
            let mut routes = route_remote_file(remote_file);
            routes.sort();
            routes.dedup();
            if routes.is_empty() {
                routes.push(String::new());
            }
            routes
                .into_iter()
                .enumerate()
                .map(move |(route_index, route)| {
                    let file_name = remote_file_name(remote_file);
                    let local_path = if route.is_empty() {
                        config.source.download_dir.join(file_name)
                    } else {
                        config
                            .source
                            .download_dir
                            .join(sanitize_route_dir(&route))
                            .join(file_name)
                    };
                    DownloadTarget {
                        remote_index,
                        route_index,
                        remote_file: remote_file.clone(),
                        local_path,
                    }
                })
        })
        .collect()
}

fn sanitize_route_dir(route: &str) -> String {
    route
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn remote_file_name(remote_file: &str) -> &str {
    remote_file.rsplit('/').next().unwrap_or(remote_file)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{ConnectionConfig, SourceConfig, SourceKind, SourceSection};

    use super::*;

    fn test_config_with_download_dir(download_dir: PathBuf) -> SourceConfig {
        SourceConfig {
            source: SourceSection {
                kind: SourceKind::Sftp,
                download_dir,
                remote_pattern: ".*".to_string(),
                cache_retention_days: 0,
                connect_retry: 1,
                download_retry: 1,
                download_parallel: 1,
                retry_interval_secs: 1,
                connect_timeout_secs: 1,
                read_timeout_secs: 1,
                connection: ConnectionConfig {
                    host: "localhost".to_string(),
                    port: 22,
                    username: "user".to_string(),
                    password: "password".to_string(),
                },
            },
        }
    }

    #[test]
    fn download_targets_expand_dest_table_routes() {
        let config = test_config_with_download_dir(PathBuf::from("downloads"));
        let remote_files = vec!["/remote/NRCELLDU.csv.gz".to_string()];
        let targets = download_targets(&config, &remote_files, &|_| {
            vec!["TPD_A".to_string(), "TPD_B".to_string()]
        });

        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets[0].local_path,
            PathBuf::from("downloads/tpd_a/NRCELLDU.csv.gz")
        );
        assert_eq!(
            targets[1].local_path,
            PathBuf::from("downloads/tpd_b/NRCELLDU.csv.gz")
        );
    }

    #[test]
    fn download_targets_fallback_to_legacy_location_without_routes() {
        let config = test_config_with_download_dir(PathBuf::from("downloads"));
        let remote_files = vec!["/remote/NRCELLDU.csv.gz".to_string()];
        let targets = download_targets(&config, &remote_files, &|_| Vec::new());

        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].local_path,
            PathBuf::from("downloads/NRCELLDU.csv.gz")
        );
    }

    #[test]
    fn cache_cleanup_removes_nested_files() {
        let dir = std::env::temp_dir().join(format!(
            "remote-file-source-cache-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("old.csv.part");
        fs::write(&file, b"partial").unwrap();
        let child_dir = dir.join("child");
        fs::create_dir_all(&child_dir).unwrap();
        let child_file = child_dir.join("old.csv.part");
        fs::write(&child_file, b"partial").unwrap();

        let config = test_config_with_download_dir(dir.clone());

        cleanup_download_dir(&config).unwrap();

        assert!(!file.exists());
        assert!(!child_file.exists());
        assert!(child_dir.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
