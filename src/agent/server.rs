use std::path::PathBuf;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::agent::runner::AgentRunner;
use crate::agent::store::AgentStore;
use crate::agent::tcp::AgentTcpClient;
use crate::message::InternalMessage;

pub async fn run_agent_server(
    agent_id: String,
    core_host: String,
    core_port: u16,
    core_api_base: String,
    data_dir: PathBuf,
    config_dir: Option<PathBuf>,
    reconnect_interval_ms: u64,
    reconnect_max_delay_ms: u64,
) -> Result<()> {
    let store = AgentStore::new(data_dir.clone(), config_dir, core_api_base.clone())?;

    let (tcp_msg_tx, mut tcp_msg_rx) = mpsc::channel::<InternalMessage>(100);
    let (send_to_tcp_tx, send_to_tcp_rx) = mpsc::channel::<InternalMessage>(100);

    let tcp_client = AgentTcpClient {
        agent_id: agent_id.clone(),
        core_host: core_host.clone(),
        core_port,
        reconnect_interval_ms,
        reconnect_max_delay_ms,
        msg_tx: tcp_msg_tx,
        msg_rx: send_to_tcp_rx,
    };
    tokio::spawn(async move {
        if let Err(e) = tcp_client.run().await {
            tracing::error!("TCP client exited: {e}");
        }
    });

    let runner = AgentRunner {
        agent_id: agent_id.clone(),
        core_api_base,
        http: reqwest::Client::new(),
        tcp_tx: Some(send_to_tcp_tx.clone()),
    };

    while let Some(msg) = tcp_msg_rx.recv().await {
        match msg {
            InternalMessage::DispatchTask(request) => {
                tracing::info!(task_id = %request.task_id, "Received task dispatch");
                let task_dir = data_dir.join("tasks").join(&request.task_id);
                let runner = runner.clone();
                let store = store.clone();
                tokio::spawn(async move {
                    if store.ensure_config_async(&request.config_snapshot_id, &runner.http).await.is_ok() {
                        if let Err(e) = runner.run_task(&store, request, task_dir).await {
                            tracing::warn!("Task failed: {e:#}");
                        }
                    } else {
                        tracing::error!("config download failed for task {}", request.task_id);
                    }
                });
            }
            InternalMessage::CancelTask(task_id) => {
                tracing::info!(%task_id, "Received cancel task");
            }
            InternalMessage::Shutdown => {
                tracing::info!("Received shutdown, agent exiting");
                break;
            }
            InternalMessage::ConfigSnapshotResponse(data) => {
                tracing::info!("Received config snapshot: {}", data.config_snapshot_id);
            }
            other => {
                tracing::warn!("Unhandled message: {other:?}");
            }
        }
    }

    Ok(())
}
