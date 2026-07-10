use std::path::Path;

use anyhow::{bail, Result};

use crate::agent::transfer::config::{TransferConfig, TransferProtocol};

pub trait TransferBackend {
    fn ensure_dir(&mut self, remote_dir: &str) -> Result<()>;
    fn remove_file_if_exists(&mut self, remote_path: &str) -> Result<()>;
    fn upload_file(&mut self, local_path: &Path, remote_path: &str) -> Result<()>;
    fn rename_replace(&mut self, from: &str, to: &str) -> Result<()>;
    fn create_empty_file(&mut self, remote_path: &str) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendKind {
    Ftp,
    Sftp,
}

fn backend_kind(config: &TransferConfig) -> Result<BackendKind> {
    match config.protocol {
        Some(TransferProtocol::Ftp) => Ok(BackendKind::Ftp),
        Some(TransferProtocol::Sftp) => Ok(BackendKind::Sftp),
        None => bail!("transfer.protocol is required"),
    }
}

pub fn connect_backend(config: &TransferConfig) -> Result<Box<dyn TransferBackend + Send>> {
    match backend_kind(config)? {
        BackendKind::Ftp => Ok(Box::new(crate::agent::transfer::ftp::connect(config)?)),
        BackendKind::Sftp => Ok(Box::new(crate::agent::transfer::sftp::connect(config)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_backend_from_transfer_protocol() {
        let mut config = TransferConfig::default();
        config.protocol = Some(TransferProtocol::Ftp);
        assert_eq!(backend_kind(&config).unwrap(), BackendKind::Ftp);
        config.protocol = Some(TransferProtocol::Sftp);
        assert_eq!(backend_kind(&config).unwrap(), BackendKind::Sftp);
    }

    #[test]
    fn backend_selection_requires_protocol_without_leaking_password() {
        let mut config = TransferConfig::default();
        config.password = "secret-password".to_string();

        let error = backend_kind(&config).unwrap_err().to_string();

        assert_eq!(error, "transfer.protocol is required");
        assert!(!error.contains("secret-password"));
    }
}
