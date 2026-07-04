use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use axum::{extract::State, routing::post, Json, Router};

use crate::agent::runner::AgentRunner;
use crate::agent::store::AgentStore;
use crate::core_agent_api::{AgentCapabilities, AgentHeartbeatRequest, AgentRegisterRequest, AgentStatus, ConfigUpdateRequest, TaskDispatchRequest, TaskDispatchResponse, TaskStatus};

#[derive(Clone)]
pub struct AgentState {
    pub store: AgentStore,
    pub runner: AgentRunner,
}

pub fn router(state: AgentState) -> Router {
    Router::new()
        .route("/api/tasks", post(dispatch_task))
        .route("/api/configs/update", post(update_config))
        .with_state(state)
}

async fn dispatch_task(axum::extract::State(state): axum::extract::State<AgentState>, Json(request): Json<TaskDispatchRequest>) -> Json<TaskDispatchResponse> {
    let task_id = request.task_id.clone();
    tracing::info!("[agent-server] dispatch_task task_id={task_id} strategy_id={} scan_start_time={}", request.strategy_id, request.scan_start_time);

    if !state.store.has_config_dir() {
        match state.store.ensure_config_async(&request.config_snapshot_id, &state.runner.http).await {
            Ok(path) => tracing::info!("[agent-server] config {} ready at {}", request.config_snapshot_id, path.display()),
            Err(e) => {
                tracing::error!("[agent-server] failed to download config {}: {e:#}", request.config_snapshot_id);
                return Json(TaskDispatchResponse {
                    task_id, accepted: false,
                    agent_task_state: TaskStatus::Failed,
                    reason: Some(format!("config download failed for {}: {e:#}", request.config_snapshot_id)),
                });
            }
        }
    }

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

async fn update_config(
    State(state): State<AgentState>,
    Json(request): Json<ConfigUpdateRequest>,
) -> Json<serde_json::Value> {
    let snapshot_id = request.snapshot_id.clone();
    tracing::info!("[agent-server] config update notification: {snapshot_id} hash={}", request.content_hash);

    tokio::spawn(async move {
        match state.store.ensure_config_async(&snapshot_id, &state.runner.http).await {
            Ok(path) => tracing::info!("[agent-server] config {snapshot_id} ready at {}", path.display()),
            Err(e) => tracing::warn!("[agent-server] config {snapshot_id} download failed: {e}"),
        }
    });

    Json(serde_json::json!({ "accepted": true }))
}

pub async fn run_agent_server(addr: SocketAddr, host: String, data_dir: PathBuf, core_api_base: String, agent_id: String, config_dir: Option<PathBuf>) -> Result<()> {
    let store = AgentStore::new(data_dir, config_dir, core_api_base.clone())?;
    let runner = AgentRunner::new(agent_id.clone(), core_api_base.clone());
    let state = AgentState { store, runner: runner.clone() };

    // Start HTTP server first, then register
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("[agent] listening on {addr}");
    let serve_handle = tokio::spawn(async move {
        axum::serve(listener, router(state)).await
            .expect("axum serve failed");
    });

    // Register + heartbeat loop: retry registration if Core is not yet available
    let register_url = format!("{}/agents/register", core_api_base.trim_end_matches('/'));
    let heartbeat_url = format!("{}/agents/{agent_id}/heartbeat",
        core_api_base.trim_end_matches('/'));
    let register_req = AgentRegisterRequest {
        agent_id: Some(agent_id.clone()),
        agent_name: agent_id.clone(),
        host,
        port: addr.port(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: AgentCapabilities {
            can_collect: true,
            can_parse: true,
            can_load: false,
            supported_protocols: vec!["ftp".to_string(), "sftp".to_string()],
        },
    };
    let loop_runner = runner.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        // First tick() is immediate, so first attempt is instant; subsequent ticks every 60s
        let mut registered = false;
        loop {
            interval.tick().await;
            if !registered {
                match loop_runner.http.post(&register_url).json(&register_req).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::info!("[agent] registered with Core");
                        registered = true;
                    }
                    Ok(resp) => tracing::warn!("[agent] register returned {} (will retry)", resp.status()),
                    Err(e) => tracing::warn!("[agent] register failed: {e} (will retry)"),
                }
            } else {
                let heartbeat_payload = AgentHeartbeatRequest {
                    status: AgentStatus::Online,
                    running_task_ids: vec![],
                    disk_free_bytes: None,
                };
                match loop_runner.http.post(&heartbeat_url).json(&heartbeat_payload).send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            tracing::warn!("[agent] heartbeat returned {} (will re-register)", resp.status());
                            registered = false;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[agent] heartbeat failed: {e} (will re-register)");
                        registered = false;
                    }
                }
            }
        }
    });

    serve_handle.await.ok();
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
        // Create a minimal config dir so the handler skips HTTP download
        let cfg_dir = dir.path().join("my_config");
        std::fs::create_dir_all(cfg_dir.join("rules")).unwrap();
        std::fs::write(cfg_dir.join("source.toml"), b"[source]").unwrap();

        let state = AgentState {
            store: AgentStore::new(dir.path().join("agent_data"), Some(cfg_dir), "http://127.0.0.1:18080/api".to_string()).unwrap(),
            runner: AgentRunner::new("agent_1".to_string(), "http://127.0.0.1:18080/api".to_string()),
        };
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
            "callback_base_url": "http://127.0.0.1:18080/api",
            "source_type": "sftp",
            "remote_pattern": "/path",
            "source_host": "host",
            "source_port": 22,
            "source_username": "user",
            "source_password": "pass",
            "source_connect_retry": 3,
            "source_download_retry": 3,
            "source_download_parallel": 4,
            "source_retry_interval_secs": 30,
            "source_connect_timeout_secs": 30,
            "source_read_timeout_secs": 300,
            "source_cache_retention_days": 7,
            "db_host": "",
            "db_port": 9000,
            "db_user": "",
            "db_password": "",
            "db_database": "",
            "db_table_name_case": "lower"
        });
        let response = app.oneshot(Request::builder().method("POST").uri("/api/tasks").header("content-type", "application/json").body(Body::from(body.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(dir.path().join("agent_data/tasks/task_1/task.json").exists());
    }

    #[tokio::test]
    async fn config_update_endpoint_accepts_notification() {
        let dir = tempdir().unwrap();
        let snap_dir = dir.path().join("agent_data/config_snapshots/v_test");
        std::fs::create_dir_all(snap_dir.join("rules")).unwrap();
        std::fs::write(snap_dir.join("source.toml"), b"[source]").unwrap();
        std::fs::write(snap_dir.join("mapping_dx.ini"), b"[m]").unwrap();
        std::fs::write(snap_dir.join("load.toml"), b"[l]").unwrap();
        std::fs::write(snap_dir.join("rules/a.json"), b"{}").unwrap();

        let state = AgentState {
            store: AgentStore::new(dir.path().join("agent_data"), None, "http://127.0.0.1:9/api".to_string()).unwrap(),
            runner: AgentRunner::new("agent_1".to_string(), "http://127.0.0.1:9/api".to_string()),
        };
        let app = router(state);
        let body = serde_json::json!({ "snapshot_id": "v_test", "content_hash": "sha256:000" });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/configs/update")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
