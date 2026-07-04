use std::path::PathBuf;

use anyhow::{bail, Result};
use remote_file_source::config::{ConnectionConfig, SourceConfig, SourceKind, SourceSection};

use crate::agent::result_csv::read_result_rows;
use crate::agent::store::AgentStore;
use crate::core_agent_api::{TaskDispatchRequest, TaskResultReport, TaskStatus};
use crate::load_config::LoadConfig;
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
        tracing::info!("[agent] run_task {} start", task.task_id);
        store.update_task_state(&task.task_id, TaskStatus::Running)?;
        tracing::info!("[agent] state -> RUNNING");

        let config_dir = task_dir.join("config");
        let output_dir = task_dir.join("output");
        let result = self.run_parse_and_report(store, task, task_dir, config_dir, output_dir).await;

        if let Err(e) = &result {
            tracing::error!("[agent] run_task failed: {e:#}");
        }

        result
    }

    async fn run_parse_and_report(&self, store: &AgentStore, task: TaskDispatchRequest, task_dir: PathBuf, config_dir: PathBuf, output_dir: PathBuf) -> Result<()> {
        let load_type = match task.load_type.to_ascii_lowercase().as_str() {
            "clickhouse" => LoadType::Clickhouse,
            "postgresql" => LoadType::Postgresql,
            other => {
                tracing::error!("[agent] unsupported load_type {other}");
                store.update_task_state(&task.task_id, TaskStatus::Failed)?;
                report_to_core(&self.http, &self.core_api_base, &task.task_id, &self.agent_id, TaskStatus::Failed, Vec::new()).await;
                bail!("unsupported load_type {other}")
            }
        };

        let source_config = SourceConfig {
            source: SourceSection {
                kind: match task.source_type.to_ascii_lowercase().as_str() {
                    "ftp" => SourceKind::Ftp,
                    _ => SourceKind::Sftp,
                },
                download_dir: task_dir.join("downloads"),
                remote_pattern: task.remote_pattern.clone(),
                cache_retention_days: task.source_cache_retention_days,
                connect_retry: task.source_connect_retry as usize,
                download_retry: task.source_download_retry as usize,
                download_parallel: task.source_download_parallel as usize,
                retry_interval_secs: task.source_retry_interval_secs,
                connect_timeout_secs: task.source_connect_timeout_secs,
                read_timeout_secs: task.source_read_timeout_secs,
                connection: ConnectionConfig {
                    host: task.source_host.clone(),
                    port: task.source_port,
                    username: task.source_username.clone(),
                    password: task.source_password.clone(),
                },
            },
        };

        let load_type_str = match load_type {
            LoadType::Clickhouse => "clickhouse",
            LoadType::Postgresql => "postgresql",
        };
        let load_config = LoadConfig::new(
            load_type_str,
            &task.db_host,
            task.db_port,
            &task.db_user,
            &task.db_password,
            &task.db_database,
            &task.db_table_name_case,
        );

        let opts = ParseJobOptions {
            input: None,
            source_config: Some(source_config),
            scan_start_time: Some(task.scan_start_time.clone()),
            config_dir: config_dir.clone(),
            output_dir: output_dir.clone(),
            collect_id: task.collect_id.clone(),
            load_type,
            load_config,
            output_delimiter: task.output_delimiter.clone(),
            encoding: task.encoding.clone(),
            recursive: false,
            rule_files: Vec::new(),
            rules_dir: Some(config_dir.join("rules")),
        };
        tracing::info!("[agent] run_parse_job input={:?} config={:?} output={:?}", opts.input, opts.config_dir, opts.output_dir);

        let (report_status, result_rows) = match run_parse_job(opts) {
            Ok(_summary) => {
                tracing::info!("[agent] parse_job completed OK");
                store.update_task_state(&task.task_id, TaskStatus::Succeeded)?;
                tracing::info!("[agent] state -> SUCCEEDED");

                let rows = read_result_rows(&output_dir).unwrap_or_else(|e| {
                    tracing::error!("[agent] read result.csv failed: {e:#}");
                    Vec::new()
                });
                tracing::info!("[agent] result.csv rows: {}", rows.len());

                (TaskStatus::Succeeded, rows)
            }
            Err(e) => {
                tracing::error!("[agent] parse_job failed: {e:#}");
                store.update_task_state(&task.task_id, TaskStatus::Failed)?;
                tracing::info!("[agent] state -> FAILED");
                (TaskStatus::Failed, Vec::new())
            }
        };

        report_to_core(&self.http, &self.core_api_base, &task.task_id, &self.agent_id, report_status, result_rows).await;

        Ok(())
    }
}

async fn report_to_core(http: &reqwest::Client, core_api_base: &str, task_id: &str, agent_id: &str, status: TaskStatus, result_rows: Vec<crate::core_agent_api::ResultRow>) {
    let report = TaskResultReport {
        task_id: task_id.to_string(),
        agent_id: agent_id.to_string(),
        status,
        result_rows,
    };
    let url = format!("{core_api_base}/tasks/{task_id}/result");
    tracing::info!("[agent] posting result to Core: {url}");
    if let Err(e) = http.post(&url).json(&report).send().await {
        tracing::error!("[agent] HTTP request to Core failed: {e:#}");
    }
}
