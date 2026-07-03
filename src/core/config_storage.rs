use std::path::{Path, PathBuf};
use anyhow::Result;

#[derive(Debug)]
pub struct ValidationError {
    pub errors: Vec<String>,
}

#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub config_snapshot_id: String,
    pub content_hash: String,
    pub file_count: usize,
}

#[derive(Clone, Debug)]
pub struct ConfigStorage {
    root: PathBuf,
}

impl ConfigStorage {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("versions"))?;
        let storage = Self { root };
        Ok(storage)
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn active_link(&self) -> PathBuf {
        self.root.join("active")
    }

    pub fn version_dir(&self, snapshot_id: &str) -> PathBuf {
        self.versions_dir().join(snapshot_id)
    }

    pub fn validate_and_unpack(&self, zip_data: &[u8], snapshot_id: &str) -> Result<ValidationResult> {
        // TODO: Task 2
        anyhow::bail!("not implemented yet")
    }
}
