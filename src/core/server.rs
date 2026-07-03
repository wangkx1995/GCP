use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use tokio::sync::Mutex;

use crate::core::db::CoreDb;
use crate::core_agent_api::{AgentRegisterRequest, AgentRegisterResponse, TaskResultReport};

#[derive(Clone)]
pub struct CoreState {
    pub db: Arc<Mutex<CoreDb>>,
}

pub fn router(state: CoreState) -> Router {
    Router::new()
        .route("/api/agents/register", post(register_agent))
        .route("/api/agents/:agent_id/heartbeat", post(heartbeat))
        .route("/api/config-snapshots/:config_snapshot_id", get(config_snapshot))
        .route("/api/tasks/:task_id/events", post(task_event))
        .route("/api/tasks/:task_id/result", post(task_result))
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

async fn config_snapshot() -> Json<serde_json::Value> {
    Json(serde_json::json!({"error": "config snapshot endpoint is wired but storage fetch is not implemented in this task"}))
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

pub async fn run_core_server(addr: SocketAddr, db_path: PathBuf) -> Result<()> {
    let state = CoreState { db: Arc::new(Mutex::new(CoreDb::open(db_path)?)) };
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
        let state = CoreState { db: Arc::new(Mutex::new(CoreDb::open(dir.path().join("core.db")).unwrap())) };
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
