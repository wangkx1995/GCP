use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use ssh2::{ErrorCode, Session};

use crate::agent::transfer::backend::TransferBackend;
use crate::agent::transfer::config::TransferConfig;

pub struct SftpTransferBackend {
    _session: Session,
    sftp: ssh2::Sftp,
}

pub fn connect(config: &TransferConfig) -> Result<SftpTransferBackend> {
    let address = format!("{}:{}", config.host, config.effective_port());
    let socket = address
        .to_socket_addrs()?
        .next()
        .context("SFTP address resolved to no socket")?;
    let tcp =
        TcpStream::connect_timeout(&socket, Duration::from_secs(config.connect_timeout_seconds))?;
    tcp.set_read_timeout(Some(Duration::from_secs(config.operation_timeout_seconds)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(config.operation_timeout_seconds)))?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.handshake()?;
    session
        .userauth_password(&config.username, &config.password)
        .with_context(|| format!("failed to login SFTP user={}", config.username))?;
    let sftp = session.sftp()?;
    Ok(SftpTransferBackend {
        _session: session,
        sftp,
    })
}

fn is_sftp_not_found(error: &ssh2::Error) -> bool {
    matches!(error.code(), ErrorCode::SFTP(_))
        && matches!(error.message(), "no such file" | "no such path")
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

impl TransferBackend for SftpTransferBackend {
    fn ensure_dir(&mut self, remote_dir: &str) -> Result<()> {
        for dir in remote_parent_dirs(&format!("{}/_", remote_dir.trim_end_matches('/'))) {
            let path = Path::new(&dir);
            match self.sftp.mkdir(path, 0o755) {
                Ok(()) => {}
                Err(error) => {
                    let stat = self.sftp.stat(path)?;
                    if !stat.is_dir() {
                        return Err(error.into());
                    }
                }
            }
        }
        Ok(())
    }

    fn remove_file_if_exists(&mut self, remote_path: &str) -> Result<()> {
        match self.sftp.unlink(Path::new(remote_path)) {
            Ok(()) => Ok(()),
            Err(error) if is_sftp_not_found(&error) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn upload_file(&mut self, local_path: &Path, remote_path: &str) -> Result<()> {
        let mut local = std::fs::File::open(local_path)?;
        let mut remote = self.sftp.create(Path::new(remote_path))?;
        std::io::copy(&mut local, &mut remote)?;
        Ok(())
    }

    fn rename_replace(&mut self, from: &str, to: &str) -> Result<()> {
        self.remove_file_if_exists(to)?;
        self.sftp.rename(Path::new(from), Path::new(to), None)?;
        Ok(())
    }

    fn create_empty_file(&mut self, remote_path: &str) -> Result<()> {
        let _file = self.sftp.create(Path::new(remote_path))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_remote_parent_directories_for_sftp_paths() {
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
