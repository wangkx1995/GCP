use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::agent::result_csv::read_result_rows;
use crate::agent::store::AgentStore;
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

    pub async fn run_task(&self, store: &AgentStore, task: TaskDispatchRequest, task_dir: PathBuf) -> Result<()> {
        store.update_task_state(&task.task_id, TaskStatus::Running)?;

        let config_dir = task_dir.join("config");
        let output_dir = task_dir.join("output");
        let load_type = match task.load_type.to_ascii_lowercase().as_str() {
            "clickhouse" => LoadType::Clickhouse,
            "postgresql" => LoadType::Postgresql,
            other => {
                store.update_task_state(&task.task_id, TaskStatus::Failed)?;
                bail!("unsupported load_type {other}")
            }
        };

        let source_toml = config_dir.join("source.toml");
        let use_remote = source_toml.exists();
        let parse_result = run_parse_job(ParseJobOptions {
            input: if use_remote { None } else { Some(task_dir.join("downloads")) },
            source_config: if use_remote { Some(source_toml) } else { None },
            scan_start_time: if use_remote { Some(task.scan_start_time.clone()) } else { None },
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
        });

        if let Err(e) = parse_result {
            store.update_task_state(&task.task_id, TaskStatus::Failed)?;
            return Err(e.context("parse_job failed"));
        }

        store.update_task_state(&task.task_id, TaskStatus::Succeeded)?;

        let rows = read_result_rows(&output_dir)
            .with_context(|| format!("read result.csv from {}", output_dir.display()))?;
        let report = TaskResultReport {
            task_id: task.task_id.clone(),
            agent_id: self.agent_id.clone(),
            status: TaskStatus::Succeeded,
            result_rows: rows,
        };
        self.http
            .post(format!("{}/tasks/{}/result", self.core_api_base, task.task_id))
            .json(&report)
            .send()
            .await?
            .error_for_status()
            .context("reporting result to Core")?;
        Ok(())
    }
}
