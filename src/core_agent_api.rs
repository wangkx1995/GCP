use serde::{Deserialize, Serialize};

pub mod serde_i64 {
    use serde::{Serializer, Deserializer};

    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        use serde::de::Error;
        struct I64Visitor;
        impl<'de> serde::de::Visitor<'de> for I64Visitor {
            type Value = i64;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a number or string-encoded i64")
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<i64, E> { Ok(v) }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<i64, E> { Ok(v as i64) }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<i64, E> {
                v.parse().map_err(E::custom)
            }
        }
        deserializer.deserialize_any(I64Visitor)
    }
}

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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AgentRegisterRequest {
    pub agent_id: Option<String>,
    pub agent_name: String,
    pub host: String,
    pub port: u16,
    pub version: String,
    pub capabilities: AgentCapabilities,
    pub cpu_total: Option<String>,
    pub memory_total: Option<f64>,
    pub disk_total: Option<f64>,
    pub max_thread_num: Option<i32>,
    pub fact_memory_total: Option<f64>,
    pub heartbeat_interval: Option<i32>,
    pub is_core: Option<bool>,
    pub deploy_dir: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentRegisterResponse {
    pub agent_id: String,
    pub result: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AgentHeartbeatRequest {
    pub status: AgentStatus,
    pub running_task_ids: Vec<String>,
    pub disk_free_bytes: Option<u64>,
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub disk_load: Option<f64>,
    pub thread_num: Option<i32>,
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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentInfoRow {
    #[serde(with = "serde_i64")]
    pub agent_id: i64,
    pub agent_name: String,
    pub agent_ip: String,
    pub port: i32,
    pub version: String,
    pub cpu_total: Option<String>,
    pub memory_total: Option<f64>,
    pub disk_total: Option<f64>,
    pub heartbeat_interval: Option<i32>,
    pub time_stamp: Option<String>,
    pub description: Option<String>,
    pub max_thread_num: Option<i32>,
    pub agent_isuse_flag: i32,
    pub fact_memory_total: Option<f64>,
    pub agent_alias: Option<String>,
    pub is_core: i32,
    pub agent_power: Option<f64>,
    pub host_load_limit: Option<f64>,
    pub registered_at: String,
    pub current_status: Option<String>,
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub disk_load: Option<f64>,
    pub current_thread_num: Option<i32>,
    pub last_heartbeat_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentStatusRow {
    #[serde(with = "serde_i64")]
    pub agent_id: i64,
    pub agent_name: String,
    pub status: String,
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub disk_load: Option<f64>,
    pub heartbeat_time: String,
    pub thread_num: Option<i32>,
    pub agent_alias: Option<String>,
    pub agent_power: Option<f64>,
    pub new_task_count: i64,
    pub active_task_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentDispatchCandidate {
    #[serde(with = "serde_i64")]
    pub agent_id: i64,
    pub agent_name: String,
    pub agent_alias: Option<String>,
    pub agent_isuse_flag: i32,
    pub agent_power: Option<f64>,
    pub host_load_limit: Option<f64>,
    pub current_status: Option<String>,
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub current_thread_num: Option<i32>,
    pub last_heartbeat_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentStatusHisRow {
    #[serde(with = "serde_i64")]
    pub agent_id: i64,
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub disk_load: Option<f64>,
    pub heartbeat_time: String,
    pub thread_num: Option<i32>,
    pub insert_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentGroupRow {
    #[serde(with = "serde_i64")]
    pub group_id: i64,
    pub group_name: String,
    pub agent_ids: String,
    pub description: Option<String>,
    pub time_stamp: Option<String>,
}

fn serialize_json_string<S>(value: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match serde_json::from_str::<serde_json::Value>(value) {
        Ok(v) => serde_json::Value::serialize(&v, serializer),
        Err(_) => serializer.serialize_str(value),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DataCollectorUnitRow {
    #[serde(with = "serde_i64")]
    pub id: i64,
    pub unit_name: String,
    pub config_name: String,
    pub config_version: String,
    #[serde(serialize_with = "serialize_json_string")]
    pub table_names: String,
    #[serde(serialize_with = "serialize_json_string")]
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
    pub load_type: String,
    pub output_delimiter: String,
    pub db_host: String,
    pub db_port: i64,
    pub db_user: String,
    pub db_password: String,
    pub db_database: String,
    pub db_table_name_case: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DataCollectorUnitSaveRequest {
    pub original_id: Option<String>,
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
    pub load_type: Option<String>,
    pub output_delimiter: Option<String>,
    pub db_host: Option<String>,
    pub db_port: Option<i64>,
    pub db_user: Option<String>,
    pub db_password: Option<String>,
    pub db_database: Option<String>,
    pub db_table_name_case: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CollectionStrategyRow {
    #[serde(rename = "id")]
    #[sqlx(rename = "strategy_id")]
    pub strategy_id: String,
    pub collector_name: String,
    #[serde(with = "serde_i64")]
    pub collector_id: i64,
    pub table_name: String,
    pub status: String,
    pub cron_expression: String,
    pub collect_interval: i64,
    pub data_interval: i64,
    pub delay_period: i64,
    pub data_start_time: Option<String>,
    pub data_end_time: Option<String>,
    pub execute_time: Option<String>,
    #[serde(serialize_with = "serialize_json_string")]
    pub agent_ids: String,
    pub strategy_type: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CollectionStrategyCreateRequest {
    #[serde(with = "serde_i64")]
    pub collector_id: i64,
    pub collector_name: String,
    pub table_names: Vec<String>,
    pub cron_expression: Option<String>,
    pub collect_interval: i64,
    pub data_interval: i64,
    pub delay_period: Option<i64>,
    pub data_start_time: Option<String>,
    pub data_end_time: Option<String>,
    pub execute_time: Option<String>,
    pub agent_ids: String,
    pub strategy_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CollectionStrategyUpdateRequest {
    pub cron_expression: Option<String>,
    pub collect_interval: Option<i64>,
    pub data_interval: Option<i64>,
    pub delay_period: Option<i64>,
    pub data_start_time: Option<String>,
    pub data_end_time: Option<String>,
    pub execute_time: Option<String>,
    pub agent_ids: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BatchStatusRequest {
    pub ids: Vec<String>,
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
    pub group_id: Option<String>,
    pub config_snapshot_id: String,
    pub scan_start_time: String,
    pub scan_end_time: Option<String>,
    pub collector_name: String,
    pub load_type: String,
    pub encoding: String,
    pub output_delimiter: String,
    pub timeout_seconds: u64,
    pub table_name: String,
    // Source connection (was source.toml)
    pub source_type: String,
    pub remote_pattern: String,
    pub source_host: String,
    pub source_port: u16,
    pub source_username: String,
    pub source_password: String,
    pub source_connect_retry: u64,
    pub source_download_retry: u64,
    pub source_download_parallel: u64,
    pub source_retry_interval_secs: u64,
    pub source_connect_timeout_secs: u64,
    pub source_read_timeout_secs: u64,
    pub source_cache_retention_days: u64,
    // DB connection (was load.toml)
    pub db_host: String,
    pub db_port: u16,
    pub db_user: String,
    pub db_password: String,
    pub db_database: String,
    pub db_table_name_case: String,
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
pub struct CsvResultRow {
    pub table_name: String,
    pub data_time: String,
    pub row_count: u64,
    pub success: i32,
    pub collect_time: String,
    pub task_id: String,
    pub strategy_id: String,
    pub group_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskResultReport {
    pub task_id: String,
    pub agent_id: String,
    pub status: TaskStatus,
    pub result_rows: Vec<CsvResultRow>,
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
            result_rows: vec![CsvResultRow {
                table_name: "TPD_A".to_string(),
                data_time: "2026-06-17 15:15:00".to_string(),
                row_count: 100,
                success: 1,
                collect_time: "2026-07-02 15:35:00".to_string(),
                task_id: "task_1".to_string(),
                strategy_id: "".to_string(),
                group_id: "".to_string(),
            }],
        };
        let json = serde_json::to_string(&report).unwrap();
        let decoded: TaskResultReport = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, report);
    }
}
