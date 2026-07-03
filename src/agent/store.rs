use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::core_agent_api::{TaskDispatchRequest, TaskStatus};

#[derive(Clone, Debug)]
pub struct AgentStore {
    root: PathBuf,
    config_dir: Option<PathBuf>,
}

impl AgentStore {
    pub fn new(root: PathBuf, config_dir: Option<PathBuf>) -> Result<Self> {
        std::fs::create_dir_all(root.join("tasks"))?;
        std::fs::create_dir_all(root.join("config_snapshots"))?;
        if let Some(ref cfg) = config_dir {
            if !cfg.exists() {
                anyhow::bail!("config-dir {} does not exist", cfg.display());
            }
        }
        Ok(Self { root, config_dir })
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
        self.populate_config(&task_dir)?;
        Ok(task_dir)
    }

    fn populate_config(&self, task_dir: &Path) -> Result<()> {
        let Some(ref cfg) = self.config_dir else { return Ok(()) };
        let dest = task_dir.join("config");
        for entry in std::fs::read_dir(cfg)
            .with_context(|| format!("read config-dir {}", cfg.display()))?
        {
            let entry = entry?;
            let src = entry.path();
            if src.file_name().map_or(true, |n| n == "rules") {
                if src.is_dir() {
                    for rule_entry in std::fs::read_dir(&src)
                        .with_context(|| format!("read rules dir {}", src.display()))?
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
            } else if src.is_file() {
                std::fs::copy(&src, dest.join(src.file_name().unwrap()))
                    .with_context(|| format!("copy config file {}", src.display()))?;
            }
        }
        tracing::info!("[agent-store] copied config files from {} to {}", cfg.display(), dest.display());
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
    fn persists_task_before_execution() {
        let dir = tempdir().unwrap();
        let store = AgentStore::new(dir.path().join("agent_data"), None).unwrap();
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
