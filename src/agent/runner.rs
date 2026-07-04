use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use remote_file_source::config::{ConnectionConfig, SourceConfig, SourceKind, SourceSection};

use crate::agent::result_csv::read_result_rows;
use crate::agent::store::AgentStore;
use crate::core_agent_api::{TaskDispatchRequest, TaskResultReport, TaskStatus};
use crate::load_config::{ClickHouseConfig, LoadConfig, PostgresConfig};
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
        let load_type = match task.load_type.to_ascii_lowercase().as_str() {
            "clickhouse" => LoadType::Clickhouse,
            "postgresql" => LoadType::Postgresql,
            other => {
                tracing::error!("[agent] unsupported load_type {other}");
                store.update_task_state(&task.task_id, TaskStatus::Failed)?;
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

        let load_config = match load_type {
            LoadType::Clickhouse => LoadConfig {
                clickhouse: ClickHouseConfig {
                    client: "clickhouse-client".to_string(),
                    host: task.db_host.clone(),
                    port: task.db_port,
                    user: task.db_user.clone(),
                    password: task.db_password.clone(),
                    database: task.db_database.clone(),
                    table_name_case: task.db_table_name_case.clone(),
                },
                postgresql: PostgresConfig {
                    client: "psql".to_string(),
                    host: String::new(),
                    port: 5432,
                    user: String::new(),
                    password: String::new(),
                    database: String::new(),
                },
            },
            LoadType::Postgresql => LoadConfig {
                clickhouse: ClickHouseConfig {
                    client: "clickhouse-client".to_string(),
                    host: String::new(),
                    port: 9000,
                    user: String::new(),
                    password: String::new(),
                    database: String::new(),
                    table_name_case: "lower".to_string(),
                },
                postgresql: PostgresConfig {
                    client: "psql".to_string(),
                    host: task.db_host.clone(),
                    port: task.db_port,
                    user: task.db_user.clone(),
                    password: task.db_password.clone(),
                    database: task.db_database.clone(),
                },
            },
        };

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

        let parse_result = run_parse_job(opts);

        if let Err(e) = parse_result {
            tracing::error!("[agent] parse_job failed: {e:#}");
            store.update_task_state(&task.task_id, TaskStatus::Failed)?;
            return Err(e).context("parse_job failed");
        }
        tracing::info!("[agent] parse_job completed OK");

        store.update_task_state(&task.task_id, TaskStatus::Succeeded)?;
        tracing::info!("[agent] state -> SUCCEEDED");

        let rows = read_result_rows(&output_dir).map_err(|e| {
            tracing::error!("[agent] read result.csv failed: {e:#}");
            e
        }).context("read result.csv")?;
        tracing::info!("[agent] result.csv rows: {}", rows.len());

        let report = TaskResultReport {
            task_id: task.task_id.clone(),
            agent_id: self.agent_id.clone(),
            status: TaskStatus::Succeeded,
            result_rows: rows,
        };
        let url = format!("{}/tasks/{}/result", self.core_api_base, task.task_id);
        tracing::info!("[agent] posting result to Core: {url}");
        let resp = self.http.post(&url).json(&report).send().await.map_err(|e| {
            tracing::error!("[agent] HTTP request to Core failed: {e:#}");
            e
        }).context("reporting result to Core")?;
        resp.error_for_status().map_err(|e| {
            tracing::error!("[agent] Core returned error: {e:#}");
            e
        }).context("Core rejected result")?;
        tracing::info!("[agent] result reported to Core OK");

        Ok(())
    }
}
