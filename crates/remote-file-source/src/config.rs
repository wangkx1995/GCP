use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SourceConfig {
    pub(crate) source: SourceSection,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SourceSection {
    #[serde(rename = "type")]
    pub(crate) kind: SourceKind,
    pub(crate) download_dir: PathBuf,
    pub(crate) remote_pattern: String,
    #[serde(default = "default_cache_retention_days")]
    pub(crate) cache_retention_days: u64,
    #[serde(default = "default_retry")]
    pub(crate) connect_retry: usize,
    #[serde(default = "default_retry")]
    pub(crate) download_retry: usize,
    #[serde(default = "default_download_parallel")]
    pub(crate) download_parallel: usize,
    #[serde(default = "default_retry_interval_secs")]
    pub(crate) retry_interval_secs: u64,
    #[serde(default = "default_connect_timeout_secs")]
    pub(crate) connect_timeout_secs: u64,
    #[serde(default = "default_read_timeout_secs")]
    pub(crate) read_timeout_secs: u64,
    pub(crate) connection: ConnectionConfig,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SourceKind {
    Ftp,
    Sftp,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ConnectionConfig {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: String,
    pub(crate) password: String,
}

pub(crate) fn load_source_config(path: &Path) -> Result<SourceConfig> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read source config: {}", path.display()))?;
    let config: SourceConfig = toml::from_str(&text)
        .with_context(|| format!("failed to parse source config TOML: {}", path.display()))?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &SourceConfig) -> Result<()> {
    let source = &config.source;
    if source.remote_pattern.trim().is_empty() {
        bail!("source.remote_pattern must not be empty");
    }
    if source.download_dir.as_os_str().is_empty() {
        bail!("source.download_dir must not be empty");
    }
    if source.connect_retry == 0 {
        bail!("source.connect_retry must be greater than 0");
    }
    if source.download_retry == 0 {
        bail!("source.download_retry must be greater than 0");
    }
    if source.download_parallel == 0 {
        bail!("source.download_parallel must be greater than 0");
    }
    if source.retry_interval_secs == 0 {
        bail!("source.retry_interval_secs must be greater than 0");
    }
    if source.connect_timeout_secs == 0 {
        bail!("source.connect_timeout_secs must be greater than 0");
    }
    if source.read_timeout_secs == 0 {
        bail!("source.read_timeout_secs must be greater than 0");
    }
    let conn = &source.connection;
    if conn.host.trim().is_empty() {
        bail!("source.connection.host must not be empty");
    }
    if conn.username.trim().is_empty() {
        bail!("source.connection.username must not be empty");
    }
    if conn.password.is_empty() {
        bail!("source.connection.password must not be empty");
    }
    Ok(())
}

fn default_retry() -> usize {
    3
}

fn default_cache_retention_days() -> u64 {
    7
}

fn default_download_parallel() -> usize {
    1
}

fn default_retry_interval_secs() -> u64 {
    30
}

fn default_connect_timeout_secs() -> u64 {
    30
}

fn default_read_timeout_secs() -> u64 {
    300
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> SourceConfig {
        SourceConfig {
            source: SourceSection {
                kind: SourceKind::Sftp,
                download_dir: PathBuf::from("downloads"),
                remote_pattern: ".*".to_string(),
                cache_retention_days: 7,
                connect_retry: 3,
                download_retry: 3,
                download_parallel: 1,
                retry_interval_secs: 30,
                connect_timeout_secs: 30,
                read_timeout_secs: 300,
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
    fn rejects_zero_retry_interval() {
        let mut config = valid_config();
        config.source.retry_interval_secs = 0;

        let err = validate_config(&config).unwrap_err();

        assert!(err
            .to_string()
            .contains("source.retry_interval_secs must be greater than 0"));
    }

    #[test]
    fn rejects_zero_download_parallel() {
        let mut config = valid_config();
        config.source.download_parallel = 0;

        let err = validate_config(&config).unwrap_err();

        assert!(err
            .to_string()
            .contains("source.download_parallel must be greater than 0"));
    }

    #[test]
    fn applies_expected_defaults() {
        let config: SourceConfig = toml::from_str(
            r#"
            [source]
            type = "sftp"
            download_dir = "downloads"
            remote_pattern = ".*"

            [source.connection]
            host = "localhost"
            port = 22
            username = "user"
            password = "password"
            "#,
        )
        .unwrap();

        assert_eq!(config.source.cache_retention_days, 7);
        assert_eq!(config.source.connect_retry, 3);
        assert_eq!(config.source.download_retry, 3);
        assert_eq!(config.source.download_parallel, 1);
        assert_eq!(config.source.retry_interval_secs, 30);
        assert_eq!(config.source.connect_timeout_secs, 30);
        assert_eq!(config.source.read_timeout_secs, 300);
        validate_config(&config).unwrap();
    }
}
