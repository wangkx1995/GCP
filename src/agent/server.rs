use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use axum::{routing::post, Json, Router};

use crate::agent::runner::AgentRunner;
use crate::agent::store::AgentStore;
use crate::core_agent_api::{TaskDispatchRequest, TaskDispatchResponse, TaskStatus};

#[derive(Clone)]
pub struct AgentState {
    pub store: AgentStore,
    pub runner: AgentRunner,
}

pub fn router(state: AgentState) -> Router {
    Router::new().route("/api/tasks", post(dispatch_task)).with_state(state)
}

async fn dispatch_task(axum::extract::State(state): axum::extract::State<AgentState>, Json(request): Json<TaskDispatchRequest>) -> Json<TaskDispatchResponse> {
    let task_id = request.task_id.clone();
    tracing::info!("[agent-server] dispatch_task task_id={task_id} strategy_id={} scan_start_time={}", request.strategy_id, request.scan_start_time);
    match state.store.persist_task(&request) {
        Ok(task_dir) => {
            tracing::info!("[agent-server] persisted task to {}", task_dir.display());
            let runner = state.runner.clone();
            let store = state.store.clone();
            let tid = task_id.clone();
            tokio::spawn(async move {
                if let Err(err) = runner.run_task(&store, request, task_dir).await {
                    tracing::warn!("[agent-server] task {tid} failed: {err:#}");
                } else {
                    tracing::info!("[agent-server] task {tid} completed");
                }
            });
            Json(TaskDispatchResponse { task_id, accepted: true, agent_task_state: TaskStatus::Accepted, reason: None })
        }
        Err(err) => {
            tracing::error!("[agent-server] persist_task failed: {err:#}");
            Json(TaskDispatchResponse { task_id, accepted: false, agent_task_state: TaskStatus::Failed, reason: Some(format!("{err:#}")) })
        }
    }
}

pub async fn run_agent_server(addr: SocketAddr, data_dir: PathBuf, core_api_base: String, agent_id: String) -> Result<()> {
    let state = AgentState { store: AgentStore::new(data_dir)?, runner: AgentRunner::new(agent_id, core_api_base) };
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
    async fn dispatch_task_persists_before_accepting() {
        let dir = tempdir().unwrap();
        let state = AgentState { store: AgentStore::new(dir.path().join("agent_data")).unwrap(), runner: AgentRunner::new("agent_1".to_string(), "http://127.0.0.1:9/api".to_string()) };
        let app = router(state);
        let body = serde_json::json!({
            "task_id": "task_1",
            "logical_task_key": "strategy:time:cfg",
            "strategy_id": "strategy",
            "config_snapshot_id": "cfg",
            "scan_start_time": "2026-06-17 15:15:00",
            "collect_id": "collect_1",
            "load_type": "clickhouse",
            "encoding": "UTF-8",
            "output_delimiter": "|",
            "timeout_seconds": 1800,
            "callback_base_url": "http://127.0.0.1:18080/api"
        });
        let response = app.oneshot(Request::builder().method("POST").uri("/api/tasks").header("content-type", "application/json").body(Body::from(body.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(dir.path().join("agent_data/tasks/task_1/task.json").exists());
    }
}
