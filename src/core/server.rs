use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Serialize;
use tracing::info;

use crate::core::config_storage::ConfigStorage;
use crate::core::db::CoreDb;
use crate::core_agent_api::{
    AgentRegisterRequest, AgentRegisterResponse, ConfigNamesResponse,
    DataCollectorUnitSaveRequest, NextIdResponse,
    TablesResponse, TaskDispatchRequest, TaskDispatchResponse, TaskResultReport,
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
    pub http: reqwest::Client,
    pub storage: Arc<ConfigStorage>,
}

pub fn router(state: CoreState) -> Router {
    Router::new()
        .route("/api/agents", get(list_agents))
        .route("/api/agents/register", post(register_agent))
        .route("/api/agents/:agent_id/heartbeat", post(heartbeat))
        .route("/api/config-snapshots/upload", post(upload_config_snapshot))
        .route("/api/config-snapshots", get(list_config_snapshots))
        .route("/api/config-snapshots/:id/activate", post(activate_config_snapshot))
        .route("/api/config-snapshots/:id/download", get(download_config_snapshot))
        .route("/api/config-snapshots/:id", get(get_config_snapshot_handler))
        .route("/api/tasks/:task_id/events", post(task_event))
        .route("/api/tasks/:task_id/result", post(task_result))
        .route("/api/tasks/dispatch", post(dispatch_task))
        .route("/api/results/grid", get(result_grid))
        .route("/api/data-collector-units/next-id", post(next_unit_id))
        .route("/api/data-collector-units", get(list_data_collector_units))
        .route("/api/data-collector-units/:id", put(upsert_data_collector_unit))
        .route("/api/data-collector-units/:id", delete(delete_data_collector_unit_handler))
        .route("/api/data-collector-units/config-names", get(search_config_names))
        .route("/api/data-collector-units/tables", get(tables_for_config_handler))
        .with_state(state)
}

async fn list_agents(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Response {
    match state.db.list_all_agents().await {
        Ok(agents) => ok_response(agents, "获取 Agent 列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn register_agent(
    axum::extract::State(state): axum::extract::State<CoreState>,
    Json(request): Json<AgentRegisterRequest>,
) -> Response {
    info!("[core] register_agent name={} host={}:{}", request.agent_name, request.host, request.port);
    match state.db.register_agent(&request).await {
        Ok(agent_id) => {
            info!("[core] agent registered: {agent_id}");
            ok_response(
                AgentRegisterResponse {
                    agent_id,
                    heartbeat_interval_seconds: 10,
                    task_report_interval_seconds: 10,
                },
                "Agent 注册成功",
            )
            .into_response()
        }
        Err(e) => {
            err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response()
        }
    }
}

async fn heartbeat(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
) -> Response {
    info!("[core] heartbeat agent_id={agent_id}");
    match state.db.update_agent_heartbeat(&agent_id).await {
        Ok(_) => ok_response(serde_json::json!({"accepted": true}), "心跳上报成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn upload_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    body: axum::body::Bytes,
) -> Response {
    if body.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "请求体为空").into_response();
    }
    let snapshot_id = format!("v_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"));
    let name = params
        .get("name")
        .map(|s| s.as_str())
        .unwrap_or(&snapshot_id);
    let result = match state.storage.validate_and_unpack(&body, &snapshot_id) {
        Ok(r) => r,
        Err(e) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("存储错误: {e}"))
                .into_response()
        }
    };
    if !result.valid {
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

    let agents = match state.db.list_online_agents().await {
        Ok(a) => a,
        Err(e) => {
            info!("[core] failed to list online agents: {e}");
            Vec::new()
        }
    };
    let content_hash = meta.content_hash.clone();
    let snapshot_id = id.clone();
    for agent in &agents {
        let http = state.http.clone();
        let agent_id = agent.agent_id.clone();
        let url = format!("http://{}:{}/api/configs/update", agent.host, agent.port);
        let sid = snapshot_id.clone();
        let ch = content_hash.clone();
        let body = serde_json::json!({
            "snapshot_id": &sid,
            "content_hash": &ch,
        });
        tokio::spawn(async move {
            match http.post(&url).json(&body).timeout(Duration::from_secs(5)).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!("[core] notified agent {agent_id} of config {sid}");
                    } else {
                        info!("[core] agent {agent_id} rejected config update: {}", resp.status());
                    }
                }
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

async fn task_event() -> Json<serde_json::Value> {
    Json(serde_json::json!({"accepted": true}))
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
    match state.db.result_rows_for_day(&query.strategy_id, &query.day).await {
        Ok(rows) => {
            info!("[core] result_grid found {} rows", rows.len());
            let expected_tables = rows
                .iter()
                .map(|row| row.table_name.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let grid = crate::core::grid::build_daily_grid(
                &query.day,
                query.interval_minutes.unwrap_or(15),
                &expected_tables,
                &rows,
            );
            ok_response(grid, "获取结果成功").into_response()
        }
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn task_result(
    axum::extract::State(state): axum::extract::State<CoreState>,
    Json(report): Json<TaskResultReport>,
) -> Response {
    info!(
        "[core] task_result task_id={} agent_id={} status={:?} rows={}",
        report.task_id,
        report.agent_id,
        report.status,
        report.result_rows.len()
    );
    match state.db.accept_task_result(&report).await {
        Ok(_) => {
            info!("[core] task_result accepted OK");
            ok_response(serde_json::json!({"accepted": true}), "结果已接收").into_response()
        }
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}

async fn dispatch_task(
    axum::extract::State(state): axum::extract::State<CoreState>,
    Json(request): Json<TaskDispatchRequest>,
) -> Response {
    let task_id = request.task_id.clone();
    info!(
        "[core] dispatch_task task_id={task_id} strategy_id={}",
        request.strategy_id
    );

    let (agent_id, agent_host, agent_port) = match state.db.select_online_agent().await {
        Ok(x) => x,
        Err(e) => {
            return err_response(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("没有可用的 Agent: {e}"),
            )
            .into_response()
        }
    };
    info!("[core] selected agent {agent_id} at {agent_host}:{agent_port}");

    if let Err(e) = state
        .db
        .create_task(
            &task_id,
            &request.logical_task_key,
            &request.strategy_id,
            &request.config_snapshot_id,
            &request.scan_start_time,
            &request.collect_id,
            &agent_id,
        )
        .await
    {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}"))
            .into_response();
    }

    let agent_url = format!("http://{agent_host}:{agent_port}/api/tasks");
    info!("[core] forwarding task to Agent: {agent_url}");
    let agent_resp = match state.http.post(&agent_url).json(&request).send().await {
        Ok(resp) => match resp.json::<TaskDispatchResponse>().await {
            Ok(r) => r,
            Err(e) => {
                return err_response(
                    StatusCode::BAD_GATEWAY,
                    format!("Agent 响应解析错误: {e}"),
                )
                .into_response()
            }
        },
        Err(e) => {
            return err_response(
                StatusCode::BAD_GATEWAY,
                format!("Agent 不可达: {e}"),
            )
            .into_response()
        }
    };

    if !agent_resp.accepted {
        info!("[core] Agent rejected task {task_id}: {:?}", agent_resp.reason);
    }
    info!("[core] dispatch_task done: accepted={}", agent_resp.accepted);
    ok_response(agent_resp, "任务分发成功").into_response()
}

async fn next_unit_id(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Response {
    match state.db.next_unit_id().await {
        Ok(id) => ok_response(NextIdResponse { id }, "获取 ID 成功").into_response(),
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
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(data): Json<DataCollectorUnitSaveRequest>,
) -> Response {
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

pub async fn run_core_server(addr: SocketAddr, db_path: PathBuf, storage: ConfigStorage) -> Result<()> {
    let state = CoreState {
        db: CoreDb::open(db_path).await?,
        http: reqwest::Client::new(),
        storage: Arc::new(storage),
    };

    // Background task: mark agents offline if no heartbeat for 180s
    let cleanup_db = state.db.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        // Start after first 60s
        interval.tick().await;
        loop {
            interval.tick().await;
            match cleanup_db.mark_stale_agents_offline(180).await {
                Ok(n) => {
                    if n > 0 {
                        tracing::info!("[core] marked {n} stale agent(s) offline");
                    }
                }
                Err(e) => tracing::error!("[core] cleanup stale agents failed: {e}"),
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[tokio::test]
    async fn register_agent_endpoint_returns_agent_id() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("config_storage")).unwrap();
        let state = CoreState {
            db: CoreDb::open(dir.path().join("core.db")).await.unwrap(),
            http: reqwest::Client::new(),
            storage: Arc::new(storage),
        };
        let app = router(state);
        let body = serde_json::json!({
            "agent_id": null,
            "agent_name": "agent-1",
            "host": "127.0.0.1",
            "port": 18081,
            "version": "1.0.0",
            "capabilities": {"can_collect": true, "can_parse": true, "can_load": false, "supported_protocols": ["ftp"]}
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/agents/register")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Should return 200 with ApiResponse wrapper containing data.agent_id
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], 200);
        assert!(json["data"]["agent_id"].as_str().unwrap().starts_with("agent_"));
    }
}
