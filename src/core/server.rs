use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use chrono::{NaiveDateTime, Timelike};
use cron::Schedule;
use std::str::FromStr;
use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use std::collections::HashMap;

use axum::extract::Query;

use crate::core::config_storage::ConfigStorage;
use crate::core::agent_id::compute_agent_id;
use crate::core::db::CoreDb;
use crate::core::tcp::listener::tcp_listener;
use crate::core::tcp::registry::{AgentId, ConnectionRegistry};
use crate::message::InternalMessage;
use tokio::sync::mpsc;
use crate::core_agent_api::{
    BatchStatusRequest, ConfigNamesResponse,
    CollectionStrategyCreateRequest, CollectionStrategyRow, CollectionStrategyUpdateRequest,
    DataCollectorUnitRow, DataCollectorUnitSaveRequest,
    TablesResponse, TaskDispatchRequest, TaskStatus,
};

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub data: Option<T>,
    pub status: u16,
    pub message: String,
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        let code = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (code, Json(&self)).into_response()
    }
}

pub fn ok_response<T: Serialize>(data: T, message: impl Into<String>) -> ApiResponse<T> {
    ApiResponse {
        data: Some(data),
        status: 200,
        message: message.into(),
    }
}

pub fn err_response(status: StatusCode, message: impl Into<String>) -> ApiResponse<()> {
    ApiResponse {
        data: None,
        status: status.as_u16(),
        message: message.into(),
    }
}

#[derive(Clone)]
pub struct CoreState {
    pub db: CoreDb,
    pub registry: ConnectionRegistry,
    pub to_tcp: mpsc::Sender<(AgentId, InternalMessage)>,
    pub http: reqwest::Client,
    pub storage: Arc<ConfigStorage>,
    pub periodic_cache: Arc<tokio::sync::RwLock<Vec<CollectionStrategyRow>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum StrategyCommandSource {
    Immediate,
    Periodic,
    Backfill,
}

#[derive(Clone, Debug)]
struct StrategyCommand {
    source: StrategyCommandSource,
    strategy: CollectionStrategyRow,
    unit: DataCollectorUnitRow,
    config_snapshot_id: String,
    scan_start_time: String,
    scan_end_time: Option<String>,
    table_names: Vec<String>,
    force_agent_id: Option<String>,
    force_group_id: Option<String>,
}

#[derive(Clone, Debug)]
struct TaskGroup {
    group_id: String,
    source: StrategyCommandSource,
    strategy_ids: Vec<String>,
    collector_id: i64,
    collector_name: String,
    candidate_ids: Vec<String>,
    scan_start_time: String,
    scan_end_time: Option<String>,
    table_names: Vec<String>,
    config_snapshot_id: String,
    force_agent_id: Option<String>,
    retry_count: u32,
}

fn compute_task_group_id(
    strategy_id: &str,
    collector_id: i64,
    scan_start_time: &str,
    scan_end_time: Option<&str>,
    table_names: &[String],
) -> String {
    let mut sorted_tables = table_names.to_vec();
    sorted_tables.sort();
    let input = format!(
        "{}|{}|{}|{}|{}",
        strategy_id,
        collector_id,
        scan_start_time,
        scan_end_time.unwrap_or(""),
        sorted_tables.join(",")
    );
    crate::crc64::crc64_ecma(&input).to_string()
}

pub fn router(state: CoreState) -> Router {
    Router::new()
        .route("/api/agents", get(list_agents))
        .route("/api/agents/status", get(list_agent_status_handler))
        .route("/api/agents/:id", get(get_agent_detail_handler).patch(update_agent_handler))
        .route("/api/agents/:id/status-history", get(get_agent_status_history_handler))
        .route("/api/agent-groups", get(list_agent_groups_handler).post(create_agent_group_handler))
        .route("/api/agent-groups/:id", put(update_agent_group_handler).delete(delete_agent_group_handler))
        .route("/api/config-snapshots/upload", post(upload_config_snapshot))
        .route("/api/config-snapshots", get(list_config_snapshots))
        .route("/api/config-snapshots/:id/activate", post(activate_config_snapshot))
        .route("/api/config-snapshots/:id/download", get(download_config_snapshot))
        .route("/api/config-snapshots/:id", get(get_config_snapshot_handler))
        .route("/api/tasks/dispatch", post(dispatch_task))
        .route("/api/results/grid", get(result_grid))
        .route("/api/data-collector-units", get(list_data_collector_units).put(upsert_data_collector_unit))
        .route("/api/data-collector-units/:id", delete(delete_data_collector_unit_handler))
        .route("/api/data-collector-units/config-names", get(search_config_names))
        .route("/api/data-collector-units/tables", get(tables_for_config_handler))
        .route("/api/strategies", post(create_strategies))
        .route("/api/strategies", get(list_strategies))
        .route("/api/strategies/batch-suspend", post(batch_suspend))
        .route("/api/strategies/batch-activate", post(batch_activate))
        .route("/api/strategies/:id", get(get_strategy))
        .route("/api/strategies/:id", put(update_strategy))
        .with_state(state)
}

async fn list_agents(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Response {
    match state.db.list_agents_with_status().await {
        Ok(agents) => ok_response(agents, "获取 Agent 列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub agent_alias: Option<String>,
    pub agent_isuse_flag: Option<i32>,
    pub agent_power: Option<f64>,
    pub host_load_limit: Option<f64>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct HistoryParams {
    pub limit: Option<i32>,
}

async fn get_agent_detail_handler(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
) -> Response {
    match state.db.get_agent_detail(id).await {
        Ok(Some(agent)) => ok_response(agent, "ok").into_response(),
        Ok(None) => err_response(StatusCode::NOT_FOUND, format!("Agent {id} 不存在")).into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn update_agent_handler(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateAgentRequest>,
) -> Response {
    match state.db.update_agent_info(
        id,
        body.agent_alias.as_deref(),
        body.agent_isuse_flag,
        body.agent_power,
        body.host_load_limit,
        body.description.as_deref(),
    )
    .await
    {
        Ok(()) => ok_response(serde_json::json!({}), "更新成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn list_agent_status_handler(
    State(state): State<CoreState>,
) -> Response {
    match state.db.list_agent_status().await {
        Ok(list) => ok_response(list, "ok").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn get_agent_status_history_handler(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
    Query(params): Query<HistoryParams>,
) -> Response {
    let limit = params.limit.unwrap_or(100);
    match state.db.get_status_history(id, limit).await {
        Ok(list) => ok_response(list, "ok").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn list_agent_groups_handler(
    State(state): State<CoreState>,
) -> Response {
    match state.db.list_agent_groups().await {
        Ok(list) => ok_response(list, "ok").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

#[derive(Deserialize)]
pub struct CreateGroupRequest {
    pub group_name: String,
    pub agent_ids: String,
    pub description: Option<String>,
}

async fn create_agent_group_handler(
    State(state): State<CoreState>,
    Json(body): Json<CreateGroupRequest>,
) -> Response {
    match state.db.create_agent_group(&body.group_name, &body.agent_ids, body.description.as_deref()).await {
        Ok(id) => ok_response(serde_json::json!({"group_id": id}), "创建成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

#[derive(Deserialize)]
pub struct UpdateGroupRequest {
    pub group_name: String,
    pub agent_ids: String,
    pub description: Option<String>,
}

async fn update_agent_group_handler(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateGroupRequest>,
) -> Response {
    match state.db.update_agent_group(id, &body.group_name, &body.agent_ids, body.description.as_deref()).await {
        Ok(()) => ok_response(serde_json::json!({}), "更新成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn delete_agent_group_handler(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
) -> Response {
    match state.db.delete_agent_group(id).await {
        Ok(true) => ok_response(serde_json::json!({"deleted": true}), "删除成功").into_response(),
        Ok(false) => err_response(StatusCode::NOT_FOUND, format!("分组 {id} 不存在")).into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn upload_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    body: axum::body::Bytes,
) -> Response {
    if body.is_empty() {
        tracing::warn!("[core] upload request body is empty");
        return err_response(StatusCode::BAD_REQUEST, "请求体为空").into_response();
    }
    let snapshot_id = format!("v_{}", crate::timeutil::now().format("%Y%m%d_%H%M%S"));
    let name = params
        .get("name")
        .map(|s| s.as_str())
        .unwrap_or(&snapshot_id);
    let result = match state.storage.validate_and_unpack(&body, &snapshot_id) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("[core] validate_and_unpack failed: {e}");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("存储错误: {e}"))
                .into_response()
        }
    };
    if !result.valid {
        tracing::warn!("[core] config validation failed: {}", result.errors.join("; "));
        return err_response(
            StatusCode::BAD_REQUEST,
            format!("配置校验失败: {}", result.errors.join("; ")),
        )
        .into_response();
    }
    if let Err(e) = state
        .db
        .insert_config_snapshot_meta(&snapshot_id, &result.content_hash, &snapshot_id, result.file_count, name, &result.table_names)
        .await
    {
        tracing::error!("[core] DB insert failed: {e}");
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}"))
            .into_response();
    }
    info!(
        "[core] uploaded config snapshot {snapshot_id} (name={name}, {} files, {} tables, hash={})",
        result.file_count, result.table_names.len(), result.content_hash
    );
    ok_response(
        serde_json::json!({
            "valid": true,
            "config_snapshot_id": snapshot_id,
            "name": name,
            "content_hash": result.content_hash,
            "file_count": result.file_count,
            "table_names": result.table_names,
        }),
        "配置上传成功",
    )
    .into_response()
}

async fn list_config_snapshots(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Response {
    match state.db.list_config_snapshots().await {
        Ok(list) => ok_response(list, "获取配置列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn get_config_snapshot_handler(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    match state.db.get_config_snapshot(&id).await {
        Ok(Some(meta)) => ok_response(meta, "获取配置详情成功").into_response(),
        Ok(None) => ok_response(serde_json::Value::Null, format!("配置 {id} 不存在")).into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn activate_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let _meta = match state.db.get_config_snapshot(&id).await {
        Ok(Some(m)) => m,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, format!("配置 {id} 不存在")).into_response(),
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    };

    let target = state.storage.version_dir(&id);
    if !target.exists() {
        return err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("版本目录 {} 不存在", target.display()),
        )
        .into_response();
    }

    let active = state.storage.active_link();
    let temp = active.with_extension("tmp");
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&temp);
        if let Err(e) = std::os::unix::fs::symlink(&target, &temp) {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("符号链接错误: {e}"))
                .into_response();
        }
        if let Err(e) = std::fs::rename(&temp, &active) {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("重命名错误: {e}"))
                .into_response();
        }
    }
    #[cfg(not(unix))]
    {
        if let Err(e) = std::fs::write(&active, target.to_string_lossy().as_bytes()) {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("写入错误: {e}"))
                .into_response();
        }
    }

    let meta = match state.db.activate_config_snapshot(&id).await {
        Ok(m) => m,
        Err(e) => {
            return err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("DB 错误: {e}"),
            )
            .into_response()
        }
    };
    info!("[core] activated config snapshot {id}");

    let agents = match state.db.list_agents_with_status().await {
        Ok(a) => a.into_iter().filter(|a| a.current_status.as_deref() == Some("ONLINE")).collect::<Vec<_>>(),
        Err(e) => {
            info!("[core] failed to list online agents: {e}");
            Vec::new()
        }
    };
    let snapshot_id = id.clone();
    for agent in &agents {
        let agent_id = agent.agent_id.to_string();
        let sid = snapshot_id.clone();
        let registry = state.registry.clone();
        tokio::spawn(async move {
            let msg = InternalMessage::ConfigSnapshotRequest(sid);
            match registry.send(&agent_id, &msg).await {
                Ok(_) => info!("[core] notified agent {agent_id} of config via TCP"),
                Err(e) => info!("[core] failed to notify agent {agent_id}: {e}"),
            }
        });
    }

    ok_response(
        serde_json::json!({
            "config_snapshot_id": id,
            "active": true,
            "content_hash": meta.content_hash,
            "activated_at": meta.activated_at,
        }),
        "配置已激活",
    )
    .into_response()
}

async fn download_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let vdir = state.storage.version_dir(&id);
    if !vdir.exists() {
        return Err((StatusCode::NOT_FOUND, format!("snapshot {id} not found on disk")));
    }
    let zip_data = crate::core::config_storage::create_zip_from_dir(&vdir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("zip error: {e}")))?;
    Ok(axum::response::Response::builder()
        .header("content-type", "application/zip")
        .header("content-disposition", format!("attachment; filename=\"{id}.zip\""))
        .body(Body::from(zip_data))
        .unwrap())
}

#[derive(serde::Deserialize)]
struct GridQuery {
    strategy_id: String,
    day: String,
    interval_minutes: Option<u32>,
}

async fn result_grid(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Query(query): axum::extract::Query<GridQuery>,
) -> Response {
    info!(
        "[core] result_grid strategy_id={} day={} interval={:?}",
        query.strategy_id, query.day, query.interval_minutes
    );

    let interval_from_query = query.interval_minutes.unwrap_or(15);
    let interval = match state.db.get_strategy(&query.strategy_id).await {
        Ok(Some(s)) => {
            let mins = (s.collect_interval.max(60) as u32) / 60;
            info!("[core] result_grid using strategy collect_interval={}s ({}min)", s.collect_interval, mins);
            mins
        }
        _ => interval_from_query,
    };

    match state.db.result_rows_for_day(&query.strategy_id, &query.day).await {
        Ok(rows) => {
            info!("[core] result_grid found {} rows", rows.len());
            let expected_tables = rows
                .iter()
                .map(|row| row.table_name.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let mut grid = crate::core::grid::build_daily_grid(
                &query.day,
                interval,
                &expected_tables,
                &rows,
            );

            let today = crate::timeutil::now().format("%Y-%m-%d").to_string();
            if query.day == today {
                let now = crate::timeutil::now();
                let total_minutes = now.hour() as u32 * 60 + now.minute() as u32;
                let cutoff = total_minutes.saturating_sub(interval as u32);
                let cutoff_str = format!("{:02}:{:02}:00", cutoff / 60, cutoff % 60);
                for row in &mut grid.rows {
                    for cell in &mut row.cells {
                        if cell.color == "gray" && &cell.data_time[11..] > cutoff_str.as_str() {
                            cell.color = "none".to_string();
                            cell.status = "future".to_string();
                        }
                    }
                }
            }

            ok_response(grid, "获取结果成功").into_response()
        }
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn dispatch_task(
    axum::extract::State(state): axum::extract::State<CoreState>,
    Json(request): Json<TaskDispatchRequest>,
) -> Response {
    info!(
        "[core] dispatch_task task_id={} strategy_id={}",
        request.task_id, request.strategy_id
    );

    let (agent_id_i64, _) = match state.db.select_online_agent().await {
        Ok(x) => x,
        Err(e) => {
            return err_response(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("没有可用的 Agent: {e}"),
            )
            .into_response()
        }
    };
    let agent_id = agent_id_i64.to_string();
    info!("[core] selected agent {agent_id}");

    if let Err(e) = state
        .db
        .create_task(
            &request.task_id,
            &request.logical_task_key,
            &request.strategy_id,
            &request.config_snapshot_id,
            &request.scan_start_time,
            &request.collector_name,
            &agent_id,
            &request.task_id,
            "",
        )
        .await
    {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}"))
            .into_response();
    }

    if !state.registry.is_connected(&agent_id).await {
        return err_response(StatusCode::SERVICE_UNAVAILABLE, &format!("Agent {agent_id} TCP 未连接")).into_response();
    }

    let msg = InternalMessage::DispatchTask(request.clone());
    if let Err(e) = state.registry.send(&agent_id, &msg).await {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("TCP 发送失败: {e}")).into_response();
    }

    ok_response(
        serde_json::json!({
            "task_id": request.task_id,
            "accepted": true,
            "agent_id": agent_id,
        }),
        "任务已分发",
    )
    .into_response()
}

async fn next_unit_id(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Response {
    match state.db.next_unit_id().await {
        Ok(id) => ok_response(serde_json::json!({ "id": id }), "获取 ID 成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn list_data_collector_units(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Response {
    match state.db.list_data_collector_units().await {
        Ok(list) => ok_response(list, "获取采集单元列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn upsert_data_collector_unit(
    axum::extract::State(state): axum::extract::State<CoreState>,
    Json(data): Json<DataCollectorUnitSaveRequest>,
) -> Response {
    let id = crate::crc64::crc64_ecma(&data.unit_name);
    if data.original_id.is_none() {
        if let Ok(Some(_)) = state.db.get_unit_by_id(id).await {
            return err_response(StatusCode::CONFLICT, format!("采集单元名称 '{}' 已存在", data.unit_name)).into_response();
        }
    }
    match state.db.upsert_data_collector_unit(id, &data).await {
        Ok(_) => ok_response(serde_json::json!({"id": id}), "保存成功").into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") || msg.contains("invalid") {
                err_response(StatusCode::BAD_REQUEST, msg).into_response()
            } else {
                err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response()
            }
        }
    }
}

async fn delete_data_collector_unit_handler(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Response {
    match state.db.delete_data_collector_unit(id).await {
        Ok(true) => ok_response(serde_json::json!({"deleted": true}), "删除成功").into_response(),
        Ok(false) => err_response(StatusCode::NOT_FOUND, format!("采集单元 {id} 不存在")).into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct SearchQuery {
    search: Option<String>,
}

async fn search_config_names(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
) -> Response {
    match state.db.search_active_config_names(query.search.as_deref()).await {
        Ok(names) => ok_response(ConfigNamesResponse { config_names: names }, "获取配置名称列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct ConfigNameQuery {
    config_name: String,
}

async fn tables_for_config_handler(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Query(query): axum::extract::Query<ConfigNameQuery>,
) -> Response {
    match state.db.tables_for_config(&query.config_name).await {
        Ok(tables) => ok_response(TablesResponse { tables }, "获取表名列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn create_strategies(
    State(state): State<CoreState>,
    Json(req): Json<CollectionStrategyCreateRequest>,
) -> Response {
    if req.table_names.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "table_names 不能为空").into_response();
    }
    if !["immediate", "periodic"].contains(&req.strategy_type.as_str()) {
        return err_response(StatusCode::BAD_REQUEST, "strategy_type 必须是 immediate 或 periodic").into_response();
    }

    let rows = match state.db.create_strategies(&req).await {
        Ok(rows) => rows,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    };

    if req.strategy_type == "immediate" {
        let unit = match state.db.get_unit_by_id(req.collector_id).await {
            Ok(Some(u)) => u,
            Ok(None) => {
                tracing::warn!("[create_strategies] unit not found for collector_id={}", req.collector_id);
                return ok_response(rows, "策略已创建，但采集单元不存在").into_response();
            }
            Err(e) => {
                tracing::warn!("[create_strategies] failed to get unit: {e}");
                return ok_response(rows, &format!("策略已创建，但查询采集单元失败: {e}")).into_response();
            }
        };
        let config_snapshot_id = match state.db.get_active_snapshot_id_for_config_name(&unit.config_name).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                tracing::warn!("[create_strategies] no active snapshot for config_name={}", unit.config_name);
                return ok_response(rows, "策略已创建，但未找到激活的配置快照").into_response();
            }
            Err(e) => {
                tracing::warn!("[create_strategies] failed to get snapshot: {e}");
                return ok_response(rows, &format!("策略已创建，但查询快照失败: {e}")).into_response();
            }
        };

        let table_names_joined = req.table_names.join(",");
        let batch_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let batch_raw = format!("{}_{}_{}", unit.unit_name, table_names_joined, batch_now);
        let batch_group_id = crate::crc64::crc64_ecma(&batch_raw).to_string();

        for row in &rows {
            let command = StrategyCommand {
                source: StrategyCommandSource::Immediate,
                strategy: row.clone(),
                unit: unit.clone(),
                config_snapshot_id: config_snapshot_id.clone(),
                scan_start_time: row.data_start_time.clone().unwrap_or_else(|| crate::timeutil::now().format("%Y-%m-%d %H:%M:%S").to_string()),
                scan_end_time: row.data_end_time.clone(),
                table_names: vec![row.table_name.clone()],
                force_agent_id: None,
                force_group_id: Some(batch_group_id.clone()),
            };
            match dispatch_strategy_command(&state, command).await {
                Ok(true) => tracing::info!("[create_strategies] dispatched strategy_id={}", row.strategy_id),
                Ok(false) => tracing::warn!("[create_strategies] queued retry for strategy_id={}", row.strategy_id),
                Err(e) => tracing::error!("[create_strategies] dispatch failed for strategy_id={}: {e}", row.strategy_id),
            }
        }
    }

    refresh_periodic_cache(&state).await;
    ok_response(rows, "创建成功").into_response()
}

async fn list_strategies(
    State(state): State<CoreState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let collector_name = params.get("collector_name").map(|s| s.as_str());
    let strategy_type = params.get("type").map(|s| s.as_str());
    let status = params.get("status").map(|s| s.as_str());
    match state.db.list_strategies(collector_name, strategy_type, status).await {
        Ok(rows) => ok_response(rows, "获取策略列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn get_strategy(
    State(state): State<CoreState>,
    Path(id): Path<String>,
) -> Response {
    match state.db.get_strategy(&id).await {
        Ok(Some(row)) => ok_response(row, "OK").into_response(),
        Ok(None) => err_response(StatusCode::NOT_FOUND, "策略不存在").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn update_strategy(
    State(state): State<CoreState>,
    Path(id): Path<String>,
    Json(req): Json<CollectionStrategyUpdateRequest>,
) -> Response {
    if let Ok(Some(s)) = state.db.get_strategy(&id).await {
        if s.strategy_type == "immediate" {
            return err_response(StatusCode::BAD_REQUEST, "一次性任务不可编辑").into_response();
        }
    }
    match state.db.update_strategy(&id, &req).await {
        Ok(true) => {
            refresh_periodic_cache(&state).await;
            ok_response(serde_json::json!({}), "更新成功").into_response()
        }
        Ok(false) => err_response(StatusCode::NOT_FOUND, "策略不存在").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn batch_suspend(
    State(state): State<CoreState>,
    Json(req): Json<BatchStatusRequest>,
) -> Response {
    if req.ids.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "ids 不能为空").into_response();
    }
    for id in &req.ids {
        if let Ok(Some(s)) = state.db.get_strategy(id).await {
            if s.strategy_type == "immediate" {
                return err_response(StatusCode::BAD_REQUEST, format!("一次性任务不可挂起 (ID: {})", id)).into_response();
            }
        }
    }
    match state.db.batch_suspend(&req.ids).await {
        Ok(count) => {
            refresh_periodic_cache(&state).await;
            ok_response(serde_json::json!({ "affected": count }), &format!("已挂起 {count} 条")).into_response()
        }
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn batch_activate(
    State(state): State<CoreState>,
    Json(req): Json<BatchStatusRequest>,
) -> Response {
    if req.ids.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "ids 不能为空").into_response();
    }
    for id in &req.ids {
        if let Ok(Some(s)) = state.db.get_strategy(id).await {
            if s.strategy_type == "immediate" {
                return err_response(StatusCode::BAD_REQUEST, format!("一次性任务不可激活 (ID: {})", id)).into_response();
            }
        }
    }
    match state.db.batch_activate(&req.ids).await {
        Ok(count) => {
            refresh_periodic_cache(&state).await;
            ok_response(serde_json::json!({ "affected": count }), &format!("已激活 {count} 条")).into_response()
        }
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

fn parse_candidate_ids(raw: &str) -> Result<Vec<String>> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Vec<String>>(raw) {
        Ok(ids) => Ok(ids),
        Err(_) => Ok(raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect()),
    }
}

fn build_task_dispatch_request(
    strategy: &CollectionStrategyRow,
    unit: &DataCollectorUnitRow,
    config_snapshot_id: &str,
    group_id: &str,
    task_id: String,
    logical_task_key: String,
    scan_start_time: String,
    scan_end_time: Option<String>,
) -> TaskDispatchRequest {
    TaskDispatchRequest {
        task_id,
        logical_task_key,
        strategy_id: strategy.strategy_id.clone(),
        group_id: Some(group_id.to_string()),
        config_snapshot_id: config_snapshot_id.to_string(),
        scan_start_time,
        scan_end_time,
        collector_name: unit.unit_name.clone(),
        load_type: unit.load_type.clone(),
        encoding: unit.file_encoding.clone(),
        output_delimiter: unit.output_delimiter.clone(),
        timeout_seconds: unit.task_timeout_seconds as u64,
        table_name: strategy.table_name.clone(),
        source_type: unit.source_type.clone(),
        remote_pattern: unit.remote_pattern.clone(),
        source_host: unit.host.clone(),
        source_port: unit.port as u16,
        source_username: unit.username.clone(),
        source_password: unit.password.clone(),
        source_connect_retry: unit.connect_retry as u64,
        source_download_retry: unit.download_retry as u64,
        source_download_parallel: unit.download_parallel as u64,
        source_retry_interval_secs: unit.retry_interval_secs as u64,
        source_connect_timeout_secs: unit.connect_timeout_secs as u64,
        source_read_timeout_secs: unit.read_timeout_secs as u64,
        source_cache_retention_days: unit.cache_retention_days as u64,
        db_host: unit.db_host.clone(),
        db_port: unit.db_port as u16,
        db_user: unit.db_user.clone(),
        db_password: unit.db_password.clone(),
        db_database: unit.db_database.clone(),
        db_table_name_case: unit.db_table_name_case.clone(),
    }
}

async fn dispatch_strategy_command(state: &CoreState, command: StrategyCommand) -> Result<bool> {
    if command.table_names.is_empty() {
        anyhow::bail!("strategy command has no table names");
    }
    let mut candidate_ids = parse_candidate_ids(&command.strategy.agent_ids)?;
    if candidate_ids.is_empty() {
        candidate_ids = parse_candidate_ids(&command.unit.agent_ids)?;
    }
    if candidate_ids.is_empty() {
        anyhow::bail!("strategy command has no candidate agents");
    }

    let strategy_id = command.strategy.strategy_id.clone();
    let group_id = if let Some(ref gid) = command.force_group_id {
        gid.clone()
    } else {
        compute_task_group_id(
            &strategy_id,
            command.unit.id,
            &command.scan_start_time,
            command.scan_end_time.as_deref(),
            &command.table_names,
        )
    };
    let group = TaskGroup {
        group_id: group_id.clone(),
        source: command.source.clone(),
        strategy_ids: vec![strategy_id.clone()],
        collector_id: command.unit.id,
        collector_name: command.unit.unit_name.clone(),
        candidate_ids,
        scan_start_time: command.scan_start_time.clone(),
        scan_end_time: command.scan_end_time.clone(),
        table_names: command.table_names.clone(),
        config_snapshot_id: command.config_snapshot_id.clone(),
        force_agent_id: command.force_agent_id.clone(),
        retry_count: 0,
    };

    let Some(agent_id) = select_agent_for_group(&state.db, &state.registry, &group, group.table_names.len() as i64, 150).await? else {
        let pending_groups = state.db.list_pending_retry_groups().await?;
        let current_retry_count = pending_groups.iter()
            .find(|(gid, _)| gid == &group_id)
            .map(|(_, rc)| *rc)
            .unwrap_or(0);
        if current_retry_count >= 9 {
            state.db.update_group_status(&group.group_id, "FAILED", Some("max retries exceeded")).await?;
            tracing::warn!(group_id = %group.group_id, retry_count = current_retry_count, "group exceeded max retries, marked FAILED");
            return Ok(false);
        }
        state.db.increment_group_retry(&group.group_id, &crate::timeutil::now().format("%Y-%m-%d %H:%M:%S").to_string(), "no available agent").await?;
        return Ok(false);
    };

    let now = crate::timeutil::now().format("%Y%m%d%H%M%S").to_string();
    let mut requests = Vec::new();
    for table_name in &group.table_names {
        let mut strategy = command.strategy.clone();
        strategy.table_name = table_name.clone();
        let task_id = format!("task_{}_{}_{}", strategy_id, table_name, now);
        let logical_task_key = format!("strategy_{}:{}:{}", strategy_id, group.scan_start_time, table_name);
        let request = build_task_dispatch_request(
            &strategy,
            &command.unit,
            &group.config_snapshot_id,
            &group.group_id,
            task_id.clone(),
            logical_task_key.clone(),
            group.scan_start_time.clone(),
            group.scan_end_time.clone(),
        );
        state.db.create_task(
            &task_id,
            &logical_task_key,
            &strategy_id,
            &group.config_snapshot_id,
            &group.scan_start_time,
            &command.unit.unit_name,
            "",
            &group.group_id,
            table_name,
        ).await?;
        requests.push(request);
    }

    state.db.assign_group_to_agent(&group.group_id, &agent_id).await?;
    state.db.update_group_status(&group.group_id, "DISPATCHING", None).await?;
    for request in requests {
        state.to_tcp.send((agent_id.clone(), InternalMessage::DispatchTask(request))).await?;
    }
    Ok(true)
}

async fn retry_group_dispatch(state: &CoreState, group_id: &str, retry_count: i64) -> Result<()> {
    let tasks = state.db.get_group_task_rows(group_id).await?;
    if tasks.is_empty() {
        anyhow::bail!("no tasks found for group {}", group_id);
    }

    let strategy_id = &tasks[0].2;
    let strategy = state.db.get_strategy(strategy_id).await?.ok_or_else(|| anyhow::anyhow!("strategy not found"))?;
    let unit = state.db.get_unit_by_id(strategy.collector_id).await?.ok_or_else(|| anyhow::anyhow!("unit not found"))?;

    let scan_start_time = tasks[0].4.clone();
    let config_snapshot_id = tasks[0].3.clone();

    let mut candidate_ids = parse_candidate_ids(&strategy.agent_ids)?;
    if candidate_ids.is_empty() {
        candidate_ids = parse_candidate_ids(&unit.agent_ids)?;
    }
    if candidate_ids.is_empty() {
        anyhow::bail!("no candidate agents");
    }

    let mut table_names = Vec::new();
    for task in &tasks {
        if let Some(s) = state.db.get_strategy(&task.2).await? {
            if !table_names.contains(&s.table_name) {
                table_names.push(s.table_name.clone());
            }
        }
    }

    let group = TaskGroup {
        group_id: group_id.to_string(),
        source: StrategyCommandSource::Immediate,
        strategy_ids: tasks.iter().map(|t| t.2.clone()).collect(),
        collector_id: unit.id,
        collector_name: unit.unit_name.clone(),
        candidate_ids,
        scan_start_time,
        scan_end_time: None,
        table_names,
        config_snapshot_id: config_snapshot_id.clone(),
        force_agent_id: None,
        retry_count: retry_count as u32,
    };

    let Some(agent_id) = select_agent_for_group(&state.db, &state.registry, &group, tasks.len() as i64, 150).await? else {
        if retry_count >= 9 {
            state.db.update_group_status(group_id, "FAILED", Some("max retries exceeded")).await?;
            tracing::warn!(group_id = %group_id, retry_count, "retry group exceeded max retries, marked FAILED");
            return Ok(());
        }
        let next_retry = crate::timeutil::now().format("%Y-%m-%d %H:%M:%S").to_string();
        state.db.increment_group_retry(group_id, &next_retry, "no available agent").await?;
        return Ok(());
    };

    state.db.assign_group_to_agent(group_id, &agent_id).await?;
    state.db.update_group_status(group_id, "DISPATCHING", None).await?;

    for task in &tasks {
        let task_strategy = state.db.get_strategy(&task.2).await?.ok_or_else(|| anyhow::anyhow!("strategy not found"))?;
        let request = build_task_dispatch_request(
            &task_strategy,
            &unit,
            &task.3,
            group_id,
            task.0.clone(),
            task.1.clone(),
            task.4.clone(),
            group.scan_end_time.clone(),
        );
        state.to_tcp.send((agent_id.clone(), InternalMessage::DispatchTask(request))).await?;
    }

    Ok(())
}

async fn retry_dispatch_loop(state: CoreState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        interval.tick().await;
        let pending = match state.db.list_pending_retry_groups().await {
            Ok(groups) => groups,
            Err(e) => {
                tracing::warn!(error = %e, "retry dispatch query failed");
                continue;
            }
        };
        for (group_id, retry_count) in pending {
            if retry_count >= 10 {
                let _ = state.db.update_group_status(&group_id, "FAILED", Some("max retries exceeded")).await;
                tracing::warn!(%group_id, retry_count, "group exceeded max retries, marked FAILED");
                continue;
            }
            if let Err(e) = retry_group_dispatch(&state, &group_id, retry_count).await {
                tracing::warn!(%group_id, error = %e, "retry group dispatch failed");
            }
        }
    }
}

pub async fn run_core_server(
    http_addr: SocketAddr,
    tcp_addr: SocketAddr,
    db_path: PathBuf,
    storage: ConfigStorage,
    cleanup_interval_seconds: u64,
    heartbeat_timeout_ms: u64,
) -> Result<()> {
    let db = CoreDb::open(db_path).await?;

    let registry = ConnectionRegistry::new();
    let (to_dispatch_tx, to_dispatch_rx) = mpsc::channel::<(AgentId, InternalMessage)>(50000);
    let (to_tcp_tx, to_tcp_rx) = mpsc::channel::<(AgentId, InternalMessage)>(50000);

    let state = CoreState {
        db: db.clone(),
        registry: registry.clone(),
        to_tcp: to_tcp_tx,
        http: reqwest::Client::new(),
        storage: Arc::new(storage),
        periodic_cache: Arc::new(tokio::sync::RwLock::new(Vec::new())),
    };

    // TCP listener — accepts agent connections
    tokio::spawn(tcp_listener(tcp_addr, to_dispatch_tx, registry.clone()));

    // Dispatch loop — processes messages from agents
    let db_for_loop = db.clone();
    let registry_for_loop = registry.clone();
    tokio::spawn(tcp_dispatch_loop(to_dispatch_rx, registry_for_loop, db_for_loop));

    // TCP sender — forwards messages to agents via registry
    let reg_for_sender = registry.clone();
    tokio::spawn(tcp_sender_loop(to_tcp_rx, reg_for_sender));

    // Cleanup loop — unregisters timed-out agents
    let db_for_cleanup = db.clone();
    tokio::spawn(tcp_cleanup_loop(registry.clone(), db_for_cleanup, cleanup_interval_seconds, heartbeat_timeout_ms));

    // Periodic strategy scan loop
    tokio::spawn(periodic_strategy_scan_loop(state.clone()));

    // Retry dispatch loop
    tokio::spawn(retry_dispatch_loop(state.clone()));

    // HTTP server — management APIs
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

async fn tcp_dispatch_loop(
    mut rx: mpsc::Receiver<(AgentId, InternalMessage)>,
    _registry: ConnectionRegistry,
    db: CoreDb,
) {
    while let Some((agent_id, msg)) = rx.recv().await {
        match msg {
            InternalMessage::TaskResult(report) => {
                tracing::info!(
                    agent_id = %agent_id,
                    task_id = %report.task_id,
                    rows = report.result_rows.len(),
                    status = ?report.status,
                    "收到 TaskResult"
                );
                if let Err(e) = db.accept_task_result(&report).await {
                    tracing::error!(%agent_id, task_id = %report.task_id, error = %e, "accept_task_result 失败");
                }
            }
            InternalMessage::DispatchTaskAck(ack) => {
                let status = if ack.accepted { "ACCEPTED" } else { "FAILED" };
                if let Err(e) = db.update_task_status(&ack.task_id, status, ack.reason.as_deref()).await {
                    tracing::warn!(%agent_id, task_id = %ack.task_id, error = %e, "update task ack status failed");
                }
            }
            InternalMessage::TaskEvent(event) => {
                tracing::info!(%agent_id, task_id = %event.event_id, status = ?event.status, phase = ?event.phase, "TaskEvent");
                let status = match event.status {
                    TaskStatus::Running => Some("RUNNING"),
                    TaskStatus::Failed => Some("FAILED"),
                    TaskStatus::Timeout => Some("TIMEOUT"),
                    TaskStatus::Cancelled => Some("CANCELLED"),
                    _ => None,
                };
                if let Some(status) = status {
                    if let Err(e) = db.update_task_status(&event.event_id, status, event.message.as_deref()).await {
                        tracing::warn!(%agent_id, task_id = %event.event_id, error = %e, "update task event status failed");
                    }
                }
            }
            InternalMessage::AgentRegister(mut req) => {
                let deploy_dir = req.deploy_dir.as_deref().unwrap_or("");
                let agent_id = compute_agent_id(&req.host, deploy_dir);
                req.agent_id = Some(agent_id.to_string());

                let alias = match db.get_agent_alias(agent_id).await {
                    Some(a) => Some(a),
                    None => db.compute_alias(&req.host).await,
                };

                if let Err(e) = db.upsert_agent_info(
                    agent_id, &req.agent_name, &req.host, req.port, &req.version,
                    req.cpu_total.as_deref(), req.memory_total, req.disk_total,
                    req.max_thread_num, req.fact_memory_total, req.heartbeat_interval,
                    req.is_core.unwrap_or(false), alias.as_deref(), deploy_dir,
                ).await {
                    tracing::warn!(%agent_id, error = %e, "upsert agent_info failed");
                }

                if let Err(e) = db.upsert_agent_status(agent_id, "ONLINE").await {
                    tracing::warn!(%agent_id, error = %e, "upsert agent_status failed");
                }

                tracing::info!(%agent_id, "Agent registered in DB");
            }
            InternalMessage::ConfigSnapshotRequest(snapshot_id) => {
                tracing::warn!(%agent_id, %snapshot_id, "ConfigSnapshotRequest 未实现");
            }
            InternalMessage::Heartbeat(hb) => {
                let agent_id_i64 = agent_id.parse::<i64>().unwrap_or(0);
                if let Err(e) = db.update_agent_heartbeat(
                    agent_id_i64, "ONLINE",
                    hb.cpu_load, hb.memory_load, hb.disk_load, hb.thread_num,
                ).await {
                    tracing::warn!(%agent_id, error = %e, "update heartbeat failed");
                }
                if let Err(e) = db.insert_status_his(
                    agent_id_i64,
                    hb.cpu_load, hb.memory_load, hb.disk_load, hb.thread_num,
                ).await {
                    tracing::warn!(%agent_id, error = %e, "insert status_his failed");
                }
            }
            InternalMessage::AgentDisconnected => {
                if let Ok(agent_id_i64) = agent_id.parse::<i64>() {
                    if let Err(e) = db.mark_agent_offline(agent_id_i64).await {
                        tracing::warn!(%agent_id, error = %e, "mark agent offline on disconnect failed");
                    }
                }
            }
            _ => {
                tracing::warn!(%agent_id, "dispatch_loop: 未处理消息类型");
            }
        }
    }
}

async fn tcp_sender_loop(
    mut rx: mpsc::Receiver<(AgentId, InternalMessage)>,
    registry: ConnectionRegistry,
) {
    while let Some((agent_id, msg)) = rx.recv().await {
        if let Err(e) = registry.send(&agent_id, &msg).await {
            tracing::error!(%agent_id, error = %e, "tcp_sender_loop 发送失败");
        }
    }
}

async fn tcp_cleanup_loop(registry: ConnectionRegistry, db: CoreDb, cleanup_interval_seconds: u64, heartbeat_timeout_ms: u64) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(cleanup_interval_seconds));
    loop {
        interval.tick().await;
        let timed_out = registry.check_timeouts(std::time::Duration::from_millis(heartbeat_timeout_ms)).await;
        for agent_id in timed_out {
            tracing::warn!(%agent_id, "心跳超时，注销");
            registry.unregister(&agent_id).await;
            if let Ok(agent_id_i64) = agent_id.parse::<i64>() {
                if let Err(e) = db.mark_agent_offline(agent_id_i64).await {
                    tracing::error!(%agent_id, error = %e, "mark agent offline failed");
                }
                if let Err(e) = db.mark_active_tasks_failed_for_agent(&agent_id, "agent heartbeat timeout").await {
                    tracing::error!(%agent_id, error = %e, "mark active tasks failed after heartbeat timeout failed");
                }
            }
        }
    }
}

fn cron_matches(cron: &str, now: &chrono::DateTime<chrono::FixedOffset>) -> bool {
    let expr = cron.trim();
    if expr.is_empty() {
        return true;
    }
    match Schedule::from_str(expr) {
        Ok(schedule) => schedule.includes(*now),
        Err(e) => {
            tracing::warn!(cron = %cron, error = %e, "invalid cron expression");
            false
        }
    }
}

async fn refresh_periodic_cache(state: &CoreState) {
    match state.db.list_active_periodic_strategies().await {
        Ok(rows) => {
            let mut cache = state.periodic_cache.write().await;
            *cache = rows;
        }
        Err(e) => tracing::warn!(error = %e, "refresh periodic cache failed"),
    }
}

async fn periodic_strategy_scan_loop(state: CoreState) {
    refresh_periodic_cache(&state).await;
    let mut cron_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        cron_tick.tick().await;
        let strategies = state.periodic_cache.read().await;
        if strategies.is_empty() {
            continue;
        }
        let now = crate::timeutil::now();
        for strategy in strategies.iter() {
            if !cron_matches(&strategy.cron_expression, &now) {
                continue;
            }
            let unit = match state.db.get_unit_by_id(strategy.collector_id).await {
                Ok(Some(unit)) => unit,
                Ok(None) => {
                    tracing::warn!(strategy_id = %strategy.strategy_id, collector_id = %strategy.collector_id, "periodic strategy unit not found");
                    continue;
                }
                Err(e) => {
                    tracing::warn!(strategy_id = %strategy.strategy_id, error = %e, "periodic strategy unit query failed");
                    continue;
                }
            };
            let config_snapshot_id = match state.db.get_active_snapshot_id_for_config_name(&unit.config_name).await {
                Ok(Some(id)) => id,
                Ok(None) => {
                    tracing::warn!(strategy_id = %strategy.strategy_id, config_name = %unit.config_name, "periodic strategy active snapshot not found");
                    continue;
                }
                Err(e) => {
                    tracing::warn!(strategy_id = %strategy.strategy_id, error = %e, "periodic strategy snapshot query failed");
                    continue;
                }
            };
            let now = crate::timeutil::now();
            let interval = strategy.data_interval.max(60);
            let ts = now.timestamp();
            let rem = ts % interval;
            let scan_start_time = chrono::DateTime::from_timestamp(ts - rem - interval, 0)
                .map(|t| t.naive_utc().format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| now.format("%Y-%m-%d %H:%M:%S").to_string());
            let logical_task_key = format!("strategy_{}:{}:{}", strategy.strategy_id, scan_start_time, strategy.table_name);
            match state.db.task_exists_by_logical_key(&logical_task_key).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(strategy_id = %strategy.strategy_id, error = %e, "periodic duplicate check failed");
                    continue;
                }
            }
            let command = StrategyCommand {
                source: StrategyCommandSource::Periodic,
                strategy: strategy.clone(),
                unit,
                config_snapshot_id,
                scan_start_time,
                scan_end_time: strategy.data_end_time.clone(),
                table_names: vec![strategy.table_name.clone()],
                force_agent_id: None,
                force_group_id: None,
            };
            if let Err(e) = dispatch_strategy_command(&state, command).await {
                tracing::warn!(strategy_id = %strategy.strategy_id, error = %e, "periodic strategy dispatch failed");
            }
        }
    }
}

fn score_agent(agent_power: f64, running_task_count: i64, new_task_count: i64, factor: f64) -> f64 {
    let total_task_count = (running_task_count + new_task_count).max(1) as f64;
    (agent_power / total_task_count) * factor - running_task_count as f64
}

async fn select_agent_for_group(
    db: &CoreDb,
    registry: &ConnectionRegistry,
    group: &TaskGroup,
    new_task_count: i64,
    heartbeat_timeout_seconds: i64,
) -> Result<Option<String>> {
    let candidate_ids = if let Some(force_agent_id) = &group.force_agent_id {
        vec![force_agent_id.clone()]
    } else {
        let mut ids = Vec::new();
        for raw in &group.candidate_ids {
            if let Ok(gid) = raw.parse::<i64>() {
                let expanded = db.expand_agent_group(gid).await?;
                if !expanded.is_empty() {
                    ids.extend(expanded);
                    continue;
                }
            }
            ids.push(raw.clone());
        }
        ids
    };

    let candidates = db.list_dispatch_candidates(&candidate_ids).await?;
    let now = crate::timeutil::now().naive_local();
    let mut best: Option<(String, f64)> = None;

    for candidate in candidates {
        if candidate.agent_isuse_flag != 1 {
            continue;
        }
        let agent_id = candidate.agent_id.to_string();
        if !registry.is_connected(&agent_id).await {
            continue;
        }
        if candidate.current_status.as_deref() != Some("ONLINE") {
            continue;
        }
        let Some(last_heartbeat_time) = candidate.last_heartbeat_time.as_deref() else { continue; };
        let Ok(heartbeat_time) = NaiveDateTime::parse_from_str(last_heartbeat_time, "%Y-%m-%d %H:%M:%S") else {
            tracing::warn!(agent_id = %candidate.agent_id, last_heartbeat_time, "select_agent_for_group: failed to parse heartbeat time");
            continue;
        };
        if (now - heartbeat_time).num_seconds() > heartbeat_timeout_seconds {
            continue;
        }
        let load_limit = candidate.host_load_limit.unwrap_or(90.0);
        if candidate.cpu_load.unwrap_or(0.0) >= load_limit || candidate.memory_load.unwrap_or(0.0) >= load_limit {
            continue;
        }
        let running = db.count_active_tasks_by_agent(&agent_id).await?;
        let power = candidate.agent_power.unwrap_or(1.0).max(1.0);
        if running + new_task_count > power.floor() as i64 {
            continue;
        }
        let score = score_agent(power, running, new_task_count, 1.0);
        if best.as_ref().map(|(_, current)| score > *current).unwrap_or(true) {
            best = Some((agent_id, score));
        }
    }

    Ok(best.map(|(agent_id, _)| agent_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[tokio::test]
    async fn list_agents_returns_ok() {
        let dir = tempdir().unwrap();
        let (to_tcp_tx, _) = mpsc::channel::<(AgentId, InternalMessage)>(64);
        let storage = ConfigStorage::new(dir.path().join("config_storage")).unwrap();
        let state = CoreState {
            db: CoreDb::open(dir.path().join("core.db")).await.unwrap(),
            registry: ConnectionRegistry::new(),
            to_tcp: to_tcp_tx,
            http: reqwest::Client::new(),
            storage: Arc::new(storage),
            periodic_cache: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        };
        let app = router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn task_group_id_is_stable_when_table_order_changes() {
        let a = compute_task_group_id(
            "101",
            202,
            "2026-07-08 10:00:00",
            Some("2026-07-08 10:15:00"),
            &["TPD_B".to_string(), "TPD_A".to_string()],
        );
        let b = compute_task_group_id(
            "101",
            202,
            "2026-07-08 10:00:00",
            Some("2026-07-08 10:15:00"),
            &["TPD_A".to_string(), "TPD_B".to_string()],
        );

        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn task_group_id_changes_for_different_window() {
        let a = compute_task_group_id("101", 202, "2026-07-08 10:00:00", None, &["TPD_A".to_string()]);
        let b = compute_task_group_id("101", 202, "2026-07-08 10:15:00", None, &["TPD_A".to_string()]);
        assert_ne!(a, b);
    }

    #[test]
    fn score_agent_prefers_more_available_capacity() {
        let busy = super::score_agent(2.0, 2, 1, 1.0);
        let idle = super::score_agent(4.0, 1, 1, 1.0);
        assert!(idle > busy);
    }

    #[test]
    fn parse_candidate_ids_accepts_json_array() {
        let ids = parse_candidate_ids("[\"100\",\"200\"]").unwrap();
        assert_eq!(ids, vec!["100".to_string(), "200".to_string()]);
    }
}
