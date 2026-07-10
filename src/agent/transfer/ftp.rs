use std::io::Cursor;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use suppaftp::types::{FileType, FtpError, Mode};
use suppaftp::{FtpStream, Status};

use crate::agent::transfer::backend::TransferBackend;
use crate::agent::transfer::config::TransferConfig;

pub struct FtpTransferBackend {
    stream: FtpStream,
}

pub fn connect(config: &TransferConfig) -> Result<FtpTransferBackend> {
    let address = format!("{}:{}", config.host, config.effective_port());
    let socket = address
        .to_socket_addrs()?
        .next()
        .context("FTP address resolved to no socket")?;
    let mut stream =
        FtpStream::connect_timeout(socket, Duration::from_secs(config.connect_timeout_seconds))?;
    stream
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(config.operation_timeout_seconds)))?;
    stream
        .get_ref()
        .set_write_timeout(Some(Duration::from_secs(config.operation_timeout_seconds)))?;
    stream
        .login(&config.username, &config.password)
        .with_context(|| format!("failed to login FTP user={}", config.username))?;
    stream.set_mode(Mode::Passive);
    stream.transfer_type(FileType::Binary)?;
    Ok(FtpTransferBackend { stream })
}

fn is_ftp_unavailable(error: &FtpError) -> bool {
    matches!(
        error,
        FtpError::UnexpectedResponse(response) if response.status == Status::FileUnavailable
    )
}

fn remote_parent_dirs(remote_path: &str) -> Vec<String> {
    let absolute = remote_path.starts_with('/');
    let mut parts = remote_path
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    parts.pop();

    let mut current = String::new();
    let mut dirs = Vec::new();
    for part in parts {
        if absolute {
            current.push('/');
        } else if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        dirs.push(current.clone());
    }
    dirs
}

impl TransferBackend for FtpTransferBackend {
    fn ensure_dir(&mut self, remote_dir: &str) -> Result<()> {
        let original = self.stream.pwd()?;
        let result = (|| {
            for dir in remote_parent_dirs(&format!("{}/_", remote_dir.trim_end_matches('/'))) {
                match self.stream.mkdir(&dir) {
                    Ok(()) => {}
                    Err(error) if is_ftp_unavailable(&error) => {
                        self.stream.cwd(&dir)?;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(())
        })();
        self.stream.cwd(&original)?;
        result
    }

    fn remove_file_if_exists(&mut self, remote_path: &str) -> Result<()> {
        match self.stream.rm(remote_path) {
            Ok(()) => Ok(()),
            Err(error) if is_ftp_unavailable(&error) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn upload_file(&mut self, local_path: &Path, remote_path: &str) -> Result<()> {
        let mut file = std::fs::File::open(local_path)?;
        self.stream.put_file(remote_path, &mut file)?;
        Ok(())
    }

    fn rename_replace(&mut self, from: &str, to: &str) -> Result<()> {
        self.remove_file_if_exists(to)?;
        self.stream.rename(from, to)?;
        Ok(())
    }

    fn create_empty_file(&mut self, remote_path: &str) -> Result<()> {
        let mut empty = Cursor::new(Vec::new());
        self.stream.put_file(remote_path, &mut empty)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_remote_parent_directories_for_ftp_paths() {
        assert_eq!(
            remote_parent_dirs("/core/uploads/level1/package/file.csv"),
            vec![
                "/core",
                "/core/uploads",
                "/core/uploads/level1",
                "/core/uploads/level1/package"
            ]
        );
        assert_eq!(
            remote_parent_dirs("relative/package/file.csv"),
            vec!["relative", "relative/package"]
        );
    }
}
