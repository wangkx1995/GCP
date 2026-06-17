use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

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

pub(crate) fn download_files(
    client: &RemoteClient,
    config: &SourceConfig,
    remote_files: &[String],
) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(&config.source.download_dir)?;
    cleanup_download_dir(config)?;
    let mut local_files = Vec::with_capacity(remote_files.len());
    for remote_file in remote_files {
        let local_path = config
            .source
            .download_dir
            .join(remote_file_name(remote_file));
        download_one_with_retry(client, config, remote_file, &local_path)
            .with_context(|| format!("failed to download remote file {remote_file}"))?;
        local_files.push(local_path);
    }
    Ok(local_files)
}

fn download_one_with_retry(
    client: &RemoteClient,
    config: &SourceConfig,
    remote_file: &str,
    local_path: &PathBuf,
) -> Result<()> {
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
    for entry in fs::read_dir(&config.source.download_dir).with_context(|| {
        format!(
            "failed to read download_dir for cache cleanup: {}",
            config.source.download_dir.display()
        )
    })? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                eprintln!("[source] cache cleanup skipped unreadable entry: {err:#}");
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                eprintln!(
                    "[source] cache cleanup skipped {}: failed to read file type: {err:#}",
                    path.display()
                );
                continue;
            }
        };
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

fn remote_file_name(remote_file: &str) -> &str {
    remote_file.rsplit('/').next().unwrap_or(remote_file)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{ConnectionConfig, SourceConfig, SourceKind, SourceSection};

    use super::*;

    #[test]
    fn cache_cleanup_removes_direct_files_only() {
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
        fs::write(child_dir.join("keep.csv.part"), b"partial").unwrap();

        let config = SourceConfig {
            source: SourceSection {
                kind: SourceKind::Sftp,
                download_dir: dir.clone(),
                remote_pattern: ".*".to_string(),
                cache_retention_days: 0,
                connect_retry: 1,
                download_retry: 1,
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
        };

        cleanup_download_dir(&config).unwrap();

        assert!(!file.exists());
        assert!(child_dir.join("keep.csv.part").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
