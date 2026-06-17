use std::fs::File;
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use ssh2::{Session, Sftp};

use crate::config::SourceConfig;

pub(crate) struct SftpClient {
    sftp: Mutex<Sftp>,
}

pub(crate) fn connect(config: &SourceConfig) -> Result<SftpClient> {
    let conn = &config.source.connection;
    let address = format!("{}:{}", conn.host, conn.port);
    let connect_timeout = Duration::from_secs(config.source.connect_timeout_secs);
    let read_timeout = Some(Duration::from_secs(config.source.read_timeout_secs));
    let socket_addr = address
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve SFTP address {address}"))?
        .next()
        .with_context(|| format!("no socket address resolved for SFTP {address}"))?;
    let tcp = TcpStream::connect_timeout(&socket_addr, connect_timeout)
        .with_context(|| format!("failed to connect SFTP {address}"))?;
    tcp.set_read_timeout(read_timeout)?;
    tcp.set_write_timeout(read_timeout)?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.handshake()?;
    session
        .userauth_password(&conn.username, &conn.password)
        .with_context(|| format!("failed to login SFTP user={}", conn.username))?;
    let sftp = session.sftp()?;
    Ok(SftpClient {
        sftp: Mutex::new(sftp),
    })
}

impl SftpClient {
    pub(crate) fn list_files(&self, scan_dir: &str) -> Result<Vec<String>> {
        let sftp = self.sftp.lock().expect("sftp mutex poisoned");
        sftp.stat(Path::new(scan_dir))
            .with_context(|| format!("SFTP scan dir is not accessible: {scan_dir}"))?;
        let mut files = Vec::new();
        list_recursive(&sftp, Path::new(scan_dir), &mut files)?;
        Ok(files)
    }

    pub(crate) fn download_file(&self, remote_file: &str, part_path: &Path) -> Result<()> {
        let sftp = self.sftp.lock().expect("sftp mutex poisoned");
        let mut remote = sftp
            .open(Path::new(remote_file))
            .with_context(|| format!("failed to open SFTP remote file: {remote_file}"))?;
        let mut local = File::create(part_path)
            .with_context(|| format!("failed to create local file: {}", part_path.display()))?;
        io::copy(&mut remote, &mut local).with_context(|| {
            format!(
                "failed to copy SFTP remote={} local={}",
                remote_file,
                part_path.display()
            )
        })?;
        Ok(())
    }
}

fn list_recursive(sftp: &Sftp, dir: &Path, files: &mut Vec<String>) -> Result<()> {
    for (path, stat) in sftp
        .readdir(dir)
        .with_context(|| format!("failed to list SFTP dir: {}", dir.display()))?
    {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == "." || name == ".." {
            continue;
        }
        if stat.is_dir() {
            list_recursive(sftp, &path, files)?;
        } else {
            files.push(path.to_string_lossy().to_string());
        }
    }
    Ok(())
}
