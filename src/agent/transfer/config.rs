use anyhow::{bail, Result};
use serde::Deserialize;
use std::fmt;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferProtocol {
    Ftp,
    Sftp,
}

#[derive(Clone, Deserialize)]
pub struct TransferConfig {
    #[serde(default)]
    pub enabled: bool,
    pub protocol: Option<TransferProtocol>,
    #[serde(default)]
    pub host: String,
    pub port: Option<u16>,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub remote_prefix: String,
    #[serde(default = "default_retry_count")]
    pub retry_count: usize,
    #[serde(default = "default_retry_interval_seconds")]
    pub retry_interval_seconds: u64,
    #[serde(default = "default_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
    #[serde(default = "default_operation_timeout_seconds")]
    pub operation_timeout_seconds: u64,
    #[serde(default = "default_success_retention_days")]
    pub success_retention_days: u64,
    #[serde(default = "default_cleanup_interval_hours")]
    pub cleanup_interval_hours: u64,
    #[serde(default = "default_ftp_passive")]
    pub ftp_passive: bool,
}

impl fmt::Debug for TransferConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransferConfig")
            .field("enabled", &self.enabled)
            .field("protocol", &self.protocol)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"******")
            .field("remote_prefix", &self.remote_prefix)
            .field("retry_count", &self.retry_count)
            .field("retry_interval_seconds", &self.retry_interval_seconds)
            .field("connect_timeout_seconds", &self.connect_timeout_seconds)
            .field("operation_timeout_seconds", &self.operation_timeout_seconds)
            .field("success_retention_days", &self.success_retention_days)
            .field("cleanup_interval_hours", &self.cleanup_interval_hours)
            .field("ftp_passive", &self.ftp_passive)
            .finish()
    }
}

fn default_retry_count() -> usize {
    3
}

fn default_retry_interval_seconds() -> u64 {
    5
}

fn default_connect_timeout_seconds() -> u64 {
    10
}

fn default_operation_timeout_seconds() -> u64 {
    60
}

fn default_success_retention_days() -> u64 {
    7
}

fn default_cleanup_interval_hours() -> u64 {
    24
}

fn default_ftp_passive() -> bool {
    true
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            protocol: None,
            host: String::new(),
            port: None,
            username: String::new(),
            password: String::new(),
            remote_prefix: String::new(),
            retry_count: default_retry_count(),
            retry_interval_seconds: default_retry_interval_seconds(),
            connect_timeout_seconds: default_connect_timeout_seconds(),
            operation_timeout_seconds: default_operation_timeout_seconds(),
            success_retention_days: default_success_retention_days(),
            cleanup_interval_hours: default_cleanup_interval_hours(),
            ftp_passive: default_ftp_passive(),
        }
    }
}

impl TransferConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.protocol.is_none() {
            bail!("transfer.protocol is required when transfer.enabled=true");
        }
        if self.host.trim().is_empty() {
            bail!("transfer.host must not be empty");
        }
        if self.port.unwrap_or(0) == 0 {
            bail!("transfer.port must be greater than 0");
        }
        if self.username.trim().is_empty() {
            bail!("transfer.username must not be empty");
        }
        if self.password.is_empty() {
            bail!("transfer.password must not be empty");
        }
        if self.remote_prefix.trim().is_empty() {
            bail!("transfer.remote_prefix must not be empty");
        }
        if self.retry_count == 0 {
            bail!("transfer.retry_count must be greater than 0");
        }
        if self.retry_interval_seconds == 0 {
            bail!("transfer.retry_interval_seconds must be greater than 0");
        }
        if self.connect_timeout_seconds == 0 {
            bail!("transfer.connect_timeout_seconds must be greater than 0");
        }
        if self.operation_timeout_seconds == 0 {
            bail!("transfer.operation_timeout_seconds must be greater than 0");
        }
        if self.cleanup_interval_hours == 0 {
            bail!("transfer.cleanup_interval_hours must be greater than 0");
        }
        if !self.ftp_passive {
            bail!("transfer.ftp_passive must be true when transfer.enabled=true");
        }
        Ok(())
    }

    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or(match self.protocol {
            Some(TransferProtocol::Ftp) => 21,
            Some(TransferProtocol::Sftp) | None => 22,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_enabled_config() -> TransferConfig {
        TransferConfig {
            enabled: true,
            protocol: Some(TransferProtocol::Sftp),
            host: "127.0.0.1".to_string(),
            port: Some(22),
            username: "agent".to_string(),
            password: "secret".to_string(),
            remote_prefix: "/core/uploads".to_string(),
            ..TransferConfig::default()
        }
    }

    #[test]
    fn transfer_config_is_disabled_with_expected_defaults_when_section_is_absent() {
        #[derive(Deserialize)]
        struct Root {
            #[serde(default)]
            transfer: TransferConfig,
        }

        let root: Root = toml::from_str("").unwrap();

        assert!(!root.transfer.enabled);
        assert_eq!(root.transfer.retry_count, 3);
        assert_eq!(root.transfer.retry_interval_seconds, 5);
        assert_eq!(root.transfer.connect_timeout_seconds, 10);
        assert_eq!(root.transfer.operation_timeout_seconds, 60);
        assert_eq!(root.transfer.success_retention_days, 7);
        assert_eq!(root.transfer.cleanup_interval_hours, 24);
        assert!(root.transfer.ftp_passive);
    }

    #[test]
    fn enabled_transfer_reports_the_invalid_field_name() {
        let cases: &[(&str, fn(&mut TransferConfig))] = &[
            ("transfer.protocol", |config| config.protocol = None),
            ("transfer.host", |config| config.host.clear()),
            ("transfer.port", |config| config.port = Some(0)),
            ("transfer.username", |config| config.username.clear()),
            ("transfer.password", |config| config.password.clear()),
            ("transfer.remote_prefix", |config| {
                config.remote_prefix.clear()
            }),
            ("transfer.retry_count", |config| config.retry_count = 0),
            ("transfer.retry_interval_seconds", |config| {
                config.retry_interval_seconds = 0
            }),
            ("transfer.connect_timeout_seconds", |config| {
                config.connect_timeout_seconds = 0
            }),
            ("transfer.operation_timeout_seconds", |config| {
                config.operation_timeout_seconds = 0
            }),
            ("transfer.cleanup_interval_hours", |config| {
                config.cleanup_interval_hours = 0
            }),
            ("transfer.ftp_passive", |config| config.ftp_passive = false),
        ];

        for (expected_field, invalidate) in cases {
            let mut config = valid_enabled_config();
            invalidate(&mut config);

            let error = config.validate().unwrap_err().to_string();
            assert!(
                error.contains(expected_field),
                "expected error to name {expected_field}, got: {error}"
            );
        }
    }

    #[test]
    fn disabled_transfer_ignores_empty_connection_fields() {
        TransferConfig::default().validate().unwrap();
    }

    #[test]
    fn transfer_config_debug_redacts_password() {
        let mut config = valid_enabled_config();
        config.password = "real-password-123".to_string();

        let debug = format!("{config:?}");

        assert!(!debug.contains("real-password-123"));
        assert!(debug.contains("password: \"******\""));
    }

    #[test]
    fn effective_port_uses_protocol_defaults_when_port_is_absent() {
        let ftp: TransferConfig = toml::from_str("protocol = \"ftp\"").unwrap();
        let sftp: TransferConfig = toml::from_str("protocol = \"sftp\"").unwrap();

        assert_eq!(ftp.effective_port(), 21);
        assert_eq!(sftp.effective_port(), 22);
        assert_eq!(TransferConfig::default().effective_port(), 22);
    }
}
