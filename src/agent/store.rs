use std::path::PathBuf;

use anyhow::Result;

use crate::core_agent_api::{TaskDispatchRequest, TaskStatus};

#[derive(Clone, Debug)]
pub struct AgentStore {
    root: PathBuf,
}

impl AgentStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("tasks"))?;
        std::fs::create_dir_all(root.join("config_snapshots"))?;
        Ok(Self { root })
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
        std::fs::write(task_dir.join("state.json"), serde_json::json!({"status": TaskStatus::Accepted}).to_string())?;
        Ok(task_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn persists_task_before_execution() {
        let dir = tempdir().unwrap();
        let store = AgentStore::new(dir.path().join("agent_data")).unwrap();
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
