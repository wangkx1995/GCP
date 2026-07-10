use std::path::PathBuf;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::agent::runner::AgentRunner;
use crate::agent::store::AgentStore;
use crate::agent::tcp::AgentTcpClient;
use crate::agent::transfer::{config::TransferConfig, OutputTransfer};
use crate::message::InternalMessage;

#[allow(clippy::too_many_arguments)]
pub async fn run_agent_server(
    agent_id: String,
    core_host: String,
    core_port: u16,
    core_api_base: String,
    data_dir: PathBuf,
    config_dir: Option<PathBuf>,
    reconnect_interval_ms: u64,
    reconnect_max_delay_ms: u64,
    heartbeat_interval_seconds: u64,
    transfer_config: TransferConfig,
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
        heartbeat_interval_seconds,
        msg_tx: tcp_msg_tx,
        msg_rx: send_to_tcp_rx,
    };
    tokio::spawn(async move {
        if let Err(e) = tcp_client.run().await {
            tracing::error!("TCP client exited: {e}");
        }
    });

    let output_transfer = OutputTransfer::new(transfer_config);
    let runner = AgentRunner {
        agent_id: agent_id.clone(),
        tcp_tx: send_to_tcp_tx.clone(),
        output_transfer,
    };
    let http = reqwest::Client::new();

    while let Some(msg) = tcp_msg_rx.recv().await {
        match msg {
            InternalMessage::DispatchTask(request) => {
                tracing::info!(task_id = %request.task_id, "Received task dispatch");
                let ack = crate::core_agent_api::TaskDispatchResponse {
                    task_id: request.task_id.clone(),
                    accepted: true,
                    agent_task_state: crate::core_agent_api::TaskStatus::Accepted,
                    reason: None,
                };
                if let Err(e) = send_to_tcp_tx
                    .send(InternalMessage::DispatchTaskAck(ack))
                    .await
                {
                    tracing::warn!(task_id = %request.task_id, error = %e, "failed to send dispatch ack");
                }
                let task_dir = data_dir.join("tasks").join(&request.task_id);
                let runner = runner.clone();
                let store = store.clone();
                let http = http.clone();
                tokio::spawn(async move {
                    if store
                        .ensure_config_async(&request.config_snapshot_id, &http)
                        .await
                        .is_ok()
                    {
                        if let Err(e) = store.persist_task(&request) {
                            tracing::warn!("Task persist failed: {e:#}");
                        } else if let Err(e) = runner.run_task(&store, request, task_dir).await {
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
            InternalMessage::ConfigSnapshotRequest(snapshot_id) => {
                tracing::info!(%snapshot_id, "Received config snapshot request");
                let store = store.clone();
                let http = http.clone();
                tokio::spawn(async move {
                    match store.ensure_config_async(&snapshot_id, &http).await {
                        Ok(path) => {
                            tracing::info!(%snapshot_id, path=%path.display(), "config cached")
                        }
                        Err(e) => tracing::warn!(%snapshot_id, error=%e, "config download failed"),
                    }
                });
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
