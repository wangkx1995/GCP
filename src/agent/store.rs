use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::core_agent_api::{TaskDispatchRequest, TaskStatus};

#[derive(Clone, Debug)]
pub struct AgentStore {
    root: PathBuf,
    config_dir: Option<PathBuf>,
    core_api_base: String,
}

impl AgentStore {
    pub fn new(root: PathBuf, config_dir: Option<PathBuf>, core_api_base: String) -> Result<Self> {
        std::fs::create_dir_all(root.join("tasks"))?;
        std::fs::create_dir_all(root.join("config_snapshots"))?;
        if let Some(ref cfg) = config_dir {
            if !cfg.exists() {
                anyhow::bail!("config-dir {} does not exist", cfg.display());
            }
        }
        Ok(Self { root, config_dir, core_api_base })
    }

    pub fn has_config_dir(&self) -> bool {
        self.config_dir.is_some()
    }

    pub fn ensure_config_sync(&self, snapshot_id: &str) -> Result<PathBuf> {
        let config_root = self.root.join("config_snapshots").join(snapshot_id);
        let marker = config_root.join("source.toml");
        if marker.exists() {
            tracing::info!("[agent-store] config {} already cached at {}", snapshot_id, config_root.display());
            return Ok(config_root);
        }
        anyhow::bail!("config {} not cached and async download not available in sync path; call ensure_config_async from async context", snapshot_id)
    }

    pub fn unpack_zip(&self, zip_data: Vec<u8>, dest: &Path) -> Result<()> {
        let reader = std::io::Cursor::new(&zip_data);
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| anyhow::anyhow!("invalid zip: {e}"))?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let raw_name = file.name().trim_end_matches('/').to_string();
            // Path traversal check
            let clean_name = raw_name.replace('\\', "/");
            if clean_name.contains("..") || clean_name.starts_with('/') {
                anyhow::bail!("path traversal detected: {raw_name}");
            }
            if file.is_dir() {
                std::fs::create_dir_all(dest.join(&clean_name))?;
            } else {
                let target = dest.join(&clean_name);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut content = Vec::new();
                file.read_to_end(&mut content)?;
                std::fs::write(&target, &content)?;
            }
        }
        Ok(())
    }

    async fn download_with_retry(&self, url: &str, http: &reqwest::Client) -> Result<Vec<u8>> {
        let max_attempts = 2;
        let mut last_err = None;
        for attempt in 1..=max_attempts {
            match tokio::time::timeout(Duration::from_secs(30), http.get(url).send()).await {
                Ok(Ok(resp)) => {
                    if resp.status().is_success() {
                        let body = resp.bytes().await.map_err(|e| anyhow::anyhow!("read body: {e}"))?;
                        return Ok(body.to_vec());
                    }
                    last_err = Some(anyhow::anyhow!("HTTP {}", resp.status()));
                }
                Ok(Err(e)) => last_err = Some(anyhow::anyhow!("{e}")),
                Err(_) => last_err = Some(anyhow::anyhow!("timeout after 30s")),
            }
            if attempt < max_attempts {
                tracing::warn!("[agent-store] download attempt {attempt} failed, retrying...");
            }
        }
        Err(anyhow::anyhow!("download failed after {max_attempts} attempts: {}", last_err.unwrap()))
    }

    pub async fn ensure_config_async(&self, snapshot_id: &str, http: &reqwest::Client) -> Result<PathBuf> {
        let config_root = self.root.join("config_snapshots").join(snapshot_id);
        let marker = config_root.join("source.toml");
        if marker.exists() {
            tracing::info!("[agent-store] config {} already cached", snapshot_id);
            return Ok(config_root);
        }

        // Download zip from Core
        let url = format!("{}/config-snapshots/{}/download", self.core_api_base, snapshot_id);
        let zip_data = self.download_with_retry(&url, http).await
            .with_context(|| format!("download config {snapshot_id}"))?;

        // Unpack to config_root
        std::fs::create_dir_all(&config_root)?;
        self.unpack_zip(zip_data, &config_root)
            .with_context(|| format!("unpack config {snapshot_id}"))?;

        tracing::info!("[agent-store] unpacked config {} to {}", snapshot_id, config_root.display());
        Ok(config_root)
    }

    pub fn task_dir(&self, task_id: &str) -> PathBuf {
        self.root.join("tasks").join(task_id)
    }

    pub fn persist_task(&self, request: &TaskDispatchRequest) -> Result<PathBuf> {
        let task_dir = self.task_dir(&request.task_id);
        std::fs::create_dir_all(task_dir.join("downloads"))?;
        std::fs::create_dir_all(task_dir.join("output"))?;
        std::fs::create_dir_all(task_dir.join("logs"))?;
        std::fs::create_dir_all(task_dir.join("config"))?;
        std::fs::create_dir_all(task_dir.join("config").join("rules"))?;
        std::fs::write(task_dir.join("task.json"), serde_json::to_vec_pretty(request)?)?;
        self.write_state(&task_dir, TaskStatus::Accepted)?;
        self.populate_config(&task_dir, &request.config_snapshot_id)?;
        Ok(task_dir)
    }

    fn copy_dir_recursively(src: &Path, dst: &Path) -> Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let dest_path = dst.join(entry.file_name());
            if path.is_dir() {
                Self::copy_dir_recursively(&path, &dest_path)?;
            } else if path.is_file() {
                std::fs::copy(&path, &dest_path)?;
            }
        }
        Ok(())
    }

    fn populate_config(&self, task_dir: &Path, snapshot_id: &str) -> Result<()> {
        let dest = task_dir.join("config");
        let src = if let Some(ref cfg) = self.config_dir {
            cfg.clone()
        } else {
            self.root.join("config_snapshots").join(snapshot_id)
        };
        if !src.exists() {
            return Ok(());
        }
        Self::copy_dir_recursively(&src, &dest)?;
        tracing::info!("[agent-store] config files ready at {}", dest.display());
        Ok(())
    }

    pub fn update_task_state(&self, task_id: &str, status: TaskStatus) -> Result<()> {
        let task_dir = self.task_dir(task_id);
        self.write_state(&task_dir, status)
    }

    fn write_state(&self, task_dir: &Path, status: TaskStatus) -> Result<()> {
        std::fs::write(task_dir.join("state.json"), serde_json::json!({"status": status}).to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn uses_local_cache_if_present() {
        let dir = tempdir().unwrap();
        let store = AgentStore::new(dir.path().join("agent_data"), None, "http://core/api".to_string()).unwrap();

        // Pre-populate cache
        let cache_dir = dir.path().join("agent_data/config_snapshots/cfg_v1");
        std::fs::create_dir_all(cache_dir.join("rules")).unwrap();
        std::fs::write(cache_dir.join("source.toml"), b"[source]").unwrap();
        std::fs::write(cache_dir.join("mapping_dx.ini"), b"[m]").unwrap();
        std::fs::write(cache_dir.join("load.toml"), b"[l]").unwrap();
        std::fs::write(cache_dir.join("rules/a.json"), b"{}").unwrap();

        // Run synchronously — it should find the cache and not attempt HTTP
        // We test the cache-check path; HTTP path is tested in server tests
        let result = store.ensure_config_sync("cfg_v1");
        assert!(result.is_ok());
        assert!(result.unwrap().join("source.toml").exists());
    }

    #[test]
    fn rejects_path_traversal_in_zip() {
        use std::io::Write;
        let dir = tempdir().unwrap();
        let store = AgentStore::new(dir.path().join("agent_data"), None, "http://core/api".to_string()).unwrap();

        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::FileOptions::<()>::default();
        zip.start_file("../evil.sh", opts).unwrap();
        zip.write_all(b"rm -rf /").unwrap();
        zip.finish().unwrap();

        let config_root = dir.path().join("agent_data/config_snapshots/v_bad");
        std::fs::create_dir_all(&config_root).unwrap();
        let result = store.unpack_zip(buf.into_inner(), &config_root);
        assert!(result.is_err());
        assert!(!dir.path().join("evil.sh").exists());
    }

    #[test]
    fn persists_task_before_execution() {
        let dir = tempdir().unwrap();
        let store = AgentStore::new(dir.path().join("agent_data"), None, "http://core/api".to_string()).unwrap();
        let request = TaskDispatchRequest {
            task_id: "task_1".to_string(),
            logical_task_key: "strategy:time:cfg".to_string(),
            strategy_id: "strategy".to_string(),
            group_id: None,
            config_snapshot_id: "cfg".to_string(),
            scan_start_time: "2026-06-17 15:15:00".to_string(),
            scan_end_time: None,
            collector_name: "test-unit".to_string(),
            load_type: "clickhouse".to_string(),
            encoding: "UTF-8".to_string(),
            output_delimiter: "|".to_string(),
            timeout_seconds: 1800,
            source_type: "sftp".to_string(),
            remote_pattern: "/path/{scan_start_time}".to_string(),
            source_host: "192.168.1.1".to_string(),
            source_port: 22,
            source_username: "user".to_string(),
            source_password: "pass".to_string(),
            source_connect_retry: 3,
            source_download_retry: 3,
            source_download_parallel: 4,
            source_retry_interval_secs: 30,
            source_connect_timeout_secs: 30,
            source_read_timeout_secs: 300,
            source_cache_retention_days: 7,
            db_host: "".to_string(),
            db_port: 9000,
            db_user: "".to_string(),
            db_password: "".to_string(),
            db_database: "".to_string(),
            db_table_name_case: "lower".to_string(),
            table_name: "test_table".to_string(),
        };
        let task_dir = store.persist_task(&request).unwrap();
        assert!(task_dir.join("task.json").exists());
        assert!(task_dir.join("output").is_dir());
        assert!(task_dir.join("config").is_dir());
    }
}
