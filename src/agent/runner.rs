use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::agent::result_csv::read_result_rows;
use crate::core_agent_api::{TaskDispatchRequest, TaskResultReport, TaskStatus};
use crate::parse_job::{run_parse_job, ParseJobOptions};
use crate::LoadType;

#[derive(Clone)]
pub struct AgentRunner {
    pub agent_id: String,
    pub core_api_base: String,
    pub http: reqwest::Client,
}

impl AgentRunner {
    pub fn new(agent_id: String, core_api_base: String) -> Self {
        Self { agent_id, core_api_base, http: reqwest::Client::new() }
    }

    pub async fn run_task(&self, task: TaskDispatchRequest, task_dir: PathBuf) -> Result<()> {
        let config_dir = task_dir.join("config");
        let output_dir = task_dir.join("output");
        let load_type = match task.load_type.to_ascii_lowercase().as_str() {
            "clickhouse" => LoadType::Clickhouse,
            "postgresql" => LoadType::Postgresql,
            other => bail!("unsupported load_type {other}"),
        };
        run_parse_job(ParseJobOptions {
            input: None,
            source_config: Some(config_dir.join("source.toml")),
            scan_start_time: Some(task.scan_start_time.clone()),
            config_dir: config_dir.clone(),
            output_dir: output_dir.clone(),
            collect_id: task.collect_id.clone(),
            load_type,
            load_config: config_dir.join("load.toml"),
            output_delimiter: task.output_delimiter.clone(),
            encoding: task.encoding.clone(),
            recursive: false,
            rule_files: Vec::new(),
            rules_dir: Some(config_dir.join("rules")),
        })?;
        let rows = read_result_rows(&output_dir)?;
        let report = TaskResultReport { task_id: task.task_id.clone(), agent_id: self.agent_id.clone(), status: TaskStatus::Succeeded, result_rows: rows };
        self.http.post(format!("{}/tasks/{}/result", self.core_api_base, task.task_id)).json(&report).send().await?.error_for_status()?;
        Ok(())
    }
}
