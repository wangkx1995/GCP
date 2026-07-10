use std::path::Path;

use anyhow::Result;

pub trait TransferBackend {
    fn ensure_dir(&mut self, remote_dir: &str) -> Result<()>;
    fn remove_file_if_exists(&mut self, remote_path: &str) -> Result<()>;
    fn upload_file(&mut self, local_path: &Path, remote_path: &str) -> Result<()>;
    fn rename_replace(&mut self, from: &str, to: &str) -> Result<()>;
    fn create_empty_file(&mut self, remote_path: &str) -> Result<()>;
}
