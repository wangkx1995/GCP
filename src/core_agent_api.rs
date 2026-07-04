use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentStatus {
    Online,
    Unknown,
    Offline,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Created,
    Dispatching,
    Accepted,
    Running,
    Succeeded,
    Failed,
    Timeout,
    CancelRequested,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    PreparingConfig,
    Downloading,
    Parsing,
    WritingOutput,
    ReportingResult,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentCapabilities {
    pub can_collect: bool,
    pub can_parse: bool,
    pub can_load: bool,
    pub supported_protocols: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentRegisterRequest {
    pub agent_id: Option<String>,
    pub agent_name: String,
    pub host: String,
    pub port: u16,
    pub version: String,
    pub capabilities: AgentCapabilities,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentRegisterResponse {
    pub agent_id: String,
    pub heartbeat_interval_seconds: u64,
    pub task_report_interval_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AgentHeartbeatRequest {
    pub status: AgentStatus,
    pub running_task_ids: Vec<String>,
    pub disk_free_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConfigSnapshotResponse {
    pub config_snapshot_id: String,
    pub content_hash: String,
    pub source_toml: String,
    pub mapping_dx_ini: String,
    pub load_toml: String,
    pub col_name_cut_config_ini: Option<String>,
    pub rules: Vec<RuleFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RuleFile {
    pub relative_path: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnlineAgent {
    pub agent_id: String,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent_id: String,
    pub agent_name: String,
    pub host: String,
    pub port: u16,
    pub version: String,
    pub capabilities: AgentCapabilities,
    pub status: AgentStatus,
    pub registered_at: String,
    pub last_heartbeat_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigUpdateRequest {
    pub snapshot_id: String,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DataCollectorUnitRow {
    pub id: i64,
    pub unit_name: String,
    pub config_name: String,
    pub config_version: String,
    pub table_names: String,
    pub agent_ids: String,
    pub data_interval_seconds: i64,
    pub collector_interval: i64,
    pub task_timeout_seconds: i64,
    pub source_type: String,
    pub file_encoding: String,
    pub remote_pattern: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub password: String,
    pub connect_retry: i64,
    pub download_retry: i64,
    pub download_parallel: i64,
    pub retry_interval_secs: i64,
    pub connect_timeout_secs: i64,
    pub read_timeout_secs: i64,
    pub cache_retention_days: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DataCollectorUnitSaveRequest {
    pub unit_name: String,
    pub config_name: String,
    pub table_names: String,
    pub agent_ids: String,
    pub data_interval_seconds: Option<i64>,
    pub collector_interval: Option<i64>,
    pub task_timeout_seconds: Option<i64>,
    pub source_type: Option<String>,
    pub file_encoding: Option<String>,
    pub remote_pattern: Option<String>,
    pub host: Option<String>,
    pub port: Option<i64>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub connect_retry: Option<i64>,
    pub download_retry: Option<i64>,
    pub download_parallel: Option<i64>,
    pub retry_interval_secs: Option<i64>,
    pub connect_timeout_secs: Option<i64>,
    pub read_timeout_secs: Option<i64>,
    pub cache_retention_days: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NextIdResponse {
    pub id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConfigNameItem {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigNamesResponse {
    pub config_names: Vec<ConfigNameItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TablesResponse {
    pub tables: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigSnapshotMeta {
    pub config_snapshot_id: String,
    pub content_hash: String,
    pub version_label: Option<String>,
    pub is_active: bool,
    pub file_count: usize,
    pub name: Option<String>,
    pub created_at: String,
    pub activated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskDispatchRequest {
    pub task_id: String,
    pub logical_task_key: String,
    pub strategy_id: String,
    pub config_snapshot_id: String,
    pub scan_start_time: String,
    pub collect_id: String,
    pub load_type: String,
    pub encoding: String,
    pub output_delimiter: String,
    pub timeout_seconds: u64,
    pub callback_base_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskDispatchResponse {
    pub task_id: String,
    pub accepted: bool,
    pub agent_task_state: TaskStatus,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskEventRequest {
    pub agent_id: String,
    pub event_id: String,
    pub status: TaskStatus,
    pub phase: Option<TaskPhase>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ResultRow {
    pub table_name: String,
    pub data_time: String,
    pub row_count: u64,
    pub success: i32,
    pub collect_time: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskResultReport {
    pub task_id: String,
    pub agent_id: String,
    pub status: TaskStatus,
    pub result_rows: Vec<ResultRow>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_serializes_as_screaming_snake_case() {
        let json = serde_json::to_string(&TaskStatus::CancelRequested).unwrap();
        assert_eq!(json, "\"CANCEL_REQUESTED\"");
    }

    #[test]
    fn result_report_round_trips_json() {
        let report = TaskResultReport {
            task_id: "task_1".to_string(),
            agent_id: "agent_1".to_string(),
            status: TaskStatus::Succeeded,
            result_rows: vec![ResultRow {
                table_name: "TPD_A".to_string(),
                data_time: "2026-06-17 15:15:00".to_string(),
                row_count: 100,
                success: 1,
                collect_time: "2026-07-02 15:35:00".to_string(),
            }],
        };
        let json = serde_json::to_string(&report).unwrap();
        let decoded: TaskResultReport = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, report);
    }
}
