use std::cell::RefCell;
use std::fs::File;
use std::io;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use suppaftp::FtpStream;

use crate::config::SourceConfig;

pub(crate) struct FtpClient {
    stream: RefCell<FtpStream>,
}

pub(crate) fn connect(config: &SourceConfig) -> Result<FtpClient> {
    let conn = &config.source.connection;
    let address = format!("{}:{}", conn.host, conn.port);
    let connect_timeout = Duration::from_secs(config.source.connect_timeout_secs);
    let read_timeout = Some(Duration::from_secs(config.source.read_timeout_secs));
    let socket_addr = address
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve FTP address {address}"))?
        .next()
        .with_context(|| format!("no socket address resolved for FTP {address}"))?;
    let mut stream = FtpStream::connect_timeout(socket_addr, connect_timeout)
        .with_context(|| format!("failed to connect FTP {address}"))?;
    stream.get_ref().set_read_timeout(read_timeout)?;
    stream.get_ref().set_write_timeout(read_timeout)?;
    stream
        .login(&conn.username, &conn.password)
        .with_context(|| format!("failed to login FTP user={}", conn.username))?;
    Ok(FtpClient {
        stream: RefCell::new(stream),
    })
}

impl FtpClient {
    pub(crate) fn list_files(&self, scan_dir: &str) -> Result<Vec<String>> {
        let mut stream = self.stream.borrow_mut();
        let original = stream.pwd().unwrap_or_else(|_| "/".to_string());
        stream
            .cwd(scan_dir)
            .with_context(|| format!("FTP scan dir is not accessible: {scan_dir}"))?;
        stream.cwd(&original).ok();
        let mut files = Vec::new();
        list_recursive(&mut stream, scan_dir, &mut files)?;
        Ok(files)
    }

    pub(crate) fn download_file(&self, remote_file: &str, part_path: &Path) -> Result<()> {
        let mut stream = self.stream.borrow_mut();
        let mut remote = stream
            .retr_as_stream(remote_file)
            .with_context(|| format!("failed to open FTP remote file: {remote_file}"))?;
        let mut local = File::create(part_path)
            .with_context(|| format!("failed to create local file: {}", part_path.display()))?;
        io::copy(&mut remote, &mut local).with_context(|| {
            format!(
                "failed to copy FTP remote={} local={}",
                remote_file,
                part_path.display()
            )
        })?;
        stream.finalize_retr_stream(remote)?;
        Ok(())
    }
}

fn list_recursive(stream: &mut FtpStream, dir: &str, files: &mut Vec<String>) -> Result<()> {
    let entries = stream
        .nlst(Some(dir))
        .with_context(|| format!("failed to list FTP dir: {dir}"))?;
    let original = stream.pwd().unwrap_or_else(|_| "/".to_string());
    for entry in entries {
        let path = normalize_remote_path(dir, &entry);
        if stream.cwd(&path).is_ok() {
            stream.cwd(&original).ok();
            list_recursive(stream, &path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn normalize_remote_path(dir: &str, entry: &str) -> String {
    if entry.starts_with('/') {
        entry.to_string()
    } else if dir.ends_with('/') {
        format!("{dir}{entry}")
    } else {
        format!("{dir}/{entry}")
    }
}
