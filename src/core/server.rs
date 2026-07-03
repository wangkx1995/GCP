use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    body::Body,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use tokio::sync::Mutex;

use crate::core::config_storage::ConfigStorage;
use crate::core::db::CoreDb;
use crate::core_agent_api::{
    AgentRegisterRequest, AgentRegisterResponse, ConfigSnapshotMeta, TaskDispatchRequest,
    TaskDispatchResponse, TaskResultReport,
};

#[derive(Clone)]
pub struct CoreState {
    pub db: Arc<Mutex<CoreDb>>,
    pub http: reqwest::Client,
    pub storage: Arc<ConfigStorage>,
}

pub fn router(state: CoreState) -> Router {
    Router::new()
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
        .with_state(state)
}

async fn register_agent(axum::extract::State(state): axum::extract::State<CoreState>, Json(request): Json<AgentRegisterRequest>) -> Result<Json<AgentRegisterResponse>, (StatusCode, String)> {
    tracing::info!("[core] register_agent name={} host={}:{}", request.agent_name, request.host, request.port);
    let agent_id = state.db.lock().await.register_agent(&request).map_err(|e| {
        tracing::error!("[core] register_agent DB error: {e:#}");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;
    tracing::info!("[core] agent registered: {agent_id}");
    Ok(Json(AgentRegisterResponse { agent_id, heartbeat_interval_seconds: 10, task_report_interval_seconds: 10 }))
}

async fn heartbeat() -> Json<serde_json::Value> {
    Json(serde_json::json!({"accepted": true}))
}

async fn upload_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if body.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty request body".to_string()));
    }

    let snapshot_id = format!("v_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"));
    let result = state.storage.validate_and_unpack(&body, &snapshot_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("storage error: {e}")))?;

    if !result.valid {
        return Err((StatusCode::BAD_REQUEST, serde_json::json!({
            "valid": false,
            "errors": result.errors,
            "config_snapshot_id": snapshot_id,
        }).to_string()));
    }

    state.db.lock().await.insert_config_snapshot_meta(&snapshot_id, &result.content_hash, &snapshot_id, result.file_count)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    tracing::info!("[core] uploaded config snapshot {snapshot_id} ({} files, hash={})", result.file_count, result.content_hash);
    Ok(Json(serde_json::json!({
        "valid": true,
        "config_snapshot_id": snapshot_id,
        "content_hash": result.content_hash,
        "file_count": result.file_count,
    })))
}

async fn list_config_snapshots(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Result<Json<Vec<ConfigSnapshotMeta>>, (StatusCode, String)> {
    let list = state.db.lock().await.list_config_snapshots()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
    Ok(Json(list))
}

async fn get_config_snapshot_handler(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ConfigSnapshotMeta>, (StatusCode, String)> {
    let meta = state.db.lock().await.get_config_snapshot(&id)
        .map_err(|e| {
            let msg = format!("{e:#}");
            if msg.contains("QueryReturnedNoRows") {
                (StatusCode::NOT_FOUND, format!("snapshot {id} not found"))
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
            }
        })?;
    Ok(Json(meta))
}

async fn activate_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let _meta = state.db.lock().await.get_config_snapshot(&id)
        .map_err(|_| (StatusCode::NOT_FOUND, format!("snapshot {id} not found")))?;

    let target = state.storage.version_dir(&id);
    if !target.exists() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("version dir {} missing", target.display())));
    }

    let active = state.storage.active_link();
    let temp = active.with_extension("tmp");
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&temp);
        std::os::unix::fs::symlink(&target, &temp).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("symlink error: {e}")))?;
        std::fs::rename(&temp, &active).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("rename error: {e}")))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&active, target.to_string_lossy().as_bytes())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write error: {e}")))?;
    }

    let meta = state.db.lock().await.activate_config_snapshot(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    tracing::info!("[core] activated config snapshot {id}");

    Ok(Json(serde_json::json!({
        "config_snapshot_id": id,
        "active": true,
        "content_hash": meta.content_hash,
        "activated_at": meta.activated_at,
    })))
}

async fn download_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::response::Response, (StatusCode, String)> {
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

async fn result_grid(axum::extract::State(state): axum::extract::State<CoreState>, axum::extract::Query(query): axum::extract::Query<GridQuery>) -> Result<Json<crate::core::grid::DailyGrid>, (StatusCode, String)> {
    tracing::info!("[core] result_grid strategy_id={} day={} interval={:?}", query.strategy_id, query.day, query.interval_minutes);
    let rows = state.db.lock().await.result_rows_for_day(&query.strategy_id, &query.day).map_err(|e| {
        tracing::error!("[core] result_grid DB error: {e:#}");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;
    tracing::info!("[core] result_grid found {} rows", rows.len());
    let expected_tables = rows.iter().map(|row| row.table_name.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>();
    Ok(Json(crate::core::grid::build_daily_grid(&query.day, query.interval_minutes.unwrap_or(15), &expected_tables, &rows)))
}

async fn task_result(axum::extract::State(state): axum::extract::State<CoreState>, Json(report): Json<TaskResultReport>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tracing::info!("[core] task_result task_id={} agent_id={} status={:?} rows={}", report.task_id, report.agent_id, report.status, report.result_rows.len());
    state.db.lock().await.accept_task_result(&report).map_err(|e| {
        tracing::error!("[core] accept_task_result error: {e:#}");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;
    tracing::info!("[core] task_result accepted OK");
    Ok(Json(serde_json::json!({"accepted": true})))
}

async fn dispatch_task(
    axum::extract::State(state): axum::extract::State<CoreState>,
    Json(request): Json<TaskDispatchRequest>,
) -> Result<Json<TaskDispatchResponse>, (StatusCode, String)> {
    let task_id = request.task_id.clone();
    tracing::info!("[core] dispatch_task task_id={task_id} strategy_id={}", request.strategy_id);

    let (agent_id, agent_host, agent_port) = state.db.lock().await.select_online_agent().map_err(|e| {
        tracing::error!("[core] no online agent: {e:#}");
        (StatusCode::SERVICE_UNAVAILABLE, format!("no online agent: {e}"))
    })?;
    tracing::info!("[core] selected agent {agent_id} at {agent_host}:{agent_port}");

    state.db.lock().await.create_task(
        &task_id, &request.logical_task_key, &request.strategy_id,
        &request.config_snapshot_id, &request.scan_start_time,
        &request.collect_id, &agent_id,
    ).map_err(|e| {
        tracing::error!("[core] create_task DB error: {e:#}");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let agent_url = format!("http://{agent_host}:{agent_port}/api/tasks");
    tracing::info!("[core] forwarding task to Agent: {agent_url}");
    let resp = state.http.post(&agent_url).json(&request).send().await.map_err(|e| {
        tracing::error!("[core] forward to Agent failed: {e:#}");
        (StatusCode::BAD_GATEWAY, format!("Agent unreachable: {e}"))
    })?;
    let agent_resp: TaskDispatchResponse = resp.json().await.map_err(|e| {
        tracing::error!("[core] Agent response parse failed: {e:#}");
        (StatusCode::BAD_GATEWAY, format!("Agent response error: {e}"))
    })?;

    if !agent_resp.accepted {
        tracing::warn!("[core] Agent rejected task {task_id}: {:?}", agent_resp.reason);
    }
    tracing::info!("[core] dispatch_task done: accepted={}", agent_resp.accepted);
    Ok(Json(agent_resp))
}

pub async fn run_core_server(addr: SocketAddr, db_path: PathBuf, storage: ConfigStorage) -> Result<()> {
    let state = CoreState {
        db: Arc::new(Mutex::new(CoreDb::open(db_path)?)),
        http: reqwest::Client::new(),
        storage: Arc::new(storage),
    };
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    use tempfile::tempdir;

    #[tokio::test]
    async fn register_agent_endpoint_returns_agent_id() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("config_storage")).unwrap();
        let state = CoreState {
            db: Arc::new(Mutex::new(CoreDb::open(dir.path().join("core.db")).unwrap())),
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
        let response = app.oneshot(Request::builder().method("POST").uri("/api/agents/register").header("content-type", "application/json").body(Body::from(body.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
