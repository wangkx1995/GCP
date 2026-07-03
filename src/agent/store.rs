use std::io::Read;
use std::path::{Path, PathBuf};

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

    pub async fn ensure_config_async(&self, snapshot_id: &str, http: &reqwest::Client) -> Result<PathBuf> {
        let config_root = self.root.join("config_snapshots").join(snapshot_id);
        let marker = config_root.join("source.toml");
        if marker.exists() {
            tracing::info!("[agent-store] config {} already cached", snapshot_id);
            return Ok(config_root);
        }

        // Download zip from Core
        let url = format!("{}/config-snapshots/{}/download", self.core_api_base, snapshot_id);
        tracing::info!("[agent-store] downloading config {} from {}", snapshot_id, url);
        let resp = http.get(&url).send().await
            .map_err(|e| anyhow::anyhow!("download config {snapshot_id}: {e}"))?;
        if !resp.status().is_success() {
            anyhow::bail!("download config {snapshot_id}: HTTP {}", resp.status());
        }
        let zip_data = resp.bytes().await?;

        // Unpack to config_root
        std::fs::create_dir_all(&config_root)?;
        self.unpack_zip(zip_data.to_vec(), &config_root)
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
        for entry in std::fs::read_dir(&src)
            .with_context(|| format!("read config dir {}", src.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.file_name().map_or(true, |n| n == "rules") {
                if path.is_dir() {
                    for rule_entry in std::fs::read_dir(&path)
                        .with_context(|| format!("read rules dir {}", path.display()))?
                    {
                        let rule_entry = rule_entry?;
                        let rule_src = rule_entry.path();
                        if rule_src.is_file() {
                            let fname = rule_src.file_name().unwrap();
                            std::fs::copy(&rule_src, dest.join("rules").join(fname))
                                .with_context(|| format!("copy rule {}", rule_src.display()))?;
                        }
                    }
                }
            } else if path.is_file() {
                std::fs::copy(&path, dest.join(path.file_name().unwrap()))
                    .with_context(|| format!("copy config file {}", path.display()))?;
            }
        }
        tracing::info!("[agent-store] config files ready at {}", dest.display());
        Ok(())
    }

    pub fn update_task_state(&self, task_id: &str, status: TaskStatus) -> Result<()> {
        let task_dir = self.task_dir(task_id);
        self.write_state(&task_dir, status)
    }

    fn write_state(&self, task_dir: &PathBuf, status: TaskStatus) -> Result<()> {
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
            config_snapshot_id: "cfg".to_string(),
            scan_start_time: "2026-06-17 15:15:00".to_string(),
            collect_id: "collect_1".to_string(),
            load_type: "clickhouse".to_string(),
            encoding: "UTF-8".to_string(),
            output_delimiter: "|".to_string(),
            timeout_seconds: 1800,
            callback_base_url: "http://127.0.0.1:18080/api".to_string(),
        };
        let task_dir = store.persist_task(&request).unwrap();
        assert!(task_dir.join("task.json").exists());
        assert!(task_dir.join("output").is_dir());
        assert!(task_dir.join("config").is_dir());
    }
}
