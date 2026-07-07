use std::sync::Arc;
use std::thread::available_parallelism;

use anyhow::Result;
use sysinfo::{Disks, System};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};

use crate::core::tcp::protocol::{new_framed_read, new_framed_write, recv_message, send_message};
use crate::core_agent_api::*;
use crate::message::InternalMessage;

pub struct AgentTcpClient {
    pub agent_id: String,
    pub core_host: String,
    pub core_port: u16,
    pub reconnect_interval_ms: u64,
    pub reconnect_max_delay_ms: u64,
    pub heartbeat_interval_seconds: u64,
    pub msg_tx: mpsc::Sender<InternalMessage>,
    pub msg_rx: mpsc::Receiver<InternalMessage>,
}

impl AgentTcpClient {
    pub async fn run(mut self) -> Result<()> {
        let mut retry_delay = self.reconnect_interval_ms;

        let mut sys = System::new_all();
        sys.refresh_all();
        let memory_total = sys.total_memory();
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let disk_total: u64 = disks
            .iter()
            .find(|d| d.mount_point() == std::path::Path::new("/"))
            .map(|d| d.total_space())
            .unwrap_or(0);

        loop {
            let addr = format!("{}:{}", self.core_host, self.core_port);
            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    tracing::info!("Agent connected to Core: {addr}");
                    retry_delay = self.reconnect_interval_ms;

                    let local_host = stream
                        .local_addr()
                        .ok()
                        .map(|a| a.ip().to_string())
                        .unwrap_or_default();
                    let local_port = stream.local_addr().map(|a| a.port()).unwrap_or(0);

                    let (reader, writer) = stream.into_split();
                    let framed_rx = Arc::new(Mutex::new(new_framed_read(reader)));
                    let framed_tx = Arc::new(Mutex::new(new_framed_write(writer)));

                    let cpu_count = available_parallelism().map(|n| n.get() as i32).unwrap_or(1);
                    let deploy_dir = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()));
                    let agent_name = format!("{}_{}", local_host, deploy_dir.as_deref().unwrap_or("unknown"));
                    let req = AgentRegisterRequest {
                        agent_id: Some(self.agent_id.clone()),
                        agent_name,
                        host: local_host,
                        port: local_port,
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        capabilities: AgentCapabilities {
                            can_collect: true,
                            can_parse: true,
                            can_load: false,
                            supported_protocols: vec!["tcp".into()],
                        },
                        cpu_total: Some(format!("{} cores", cpu_count)),
                        memory_total: Some(memory_total as f64),
                        disk_total: Some(disk_total as f64),
                        max_thread_num: Some(cpu_count * 2),
                        fact_memory_total: Some(memory_total as f64),
                        heartbeat_interval: None,
                        is_core: None,
                        deploy_dir,
                    };
                    {
                        let mut tx = framed_tx.lock().await;
                        send_message(&mut *tx, &InternalMessage::AgentRegister(req)).await?;
                    }

                    let ack_msg = {
                        let mut rx = framed_rx.lock().await;
                        recv_message(&mut *rx).await
                    };
                    match ack_msg {
                        Ok(Some(InternalMessage::AgentRegisterAck(_))) => {
                            tracing::info!("Agent registered successfully");
                        }
                        _ => {
                            tracing::warn!("Agent registration ack invalid, reconnecting");
                            continue;
                        }
                    }

                    let hb_tx = framed_tx.clone();
                    let thread_count = cpu_count;
                    tokio::spawn(async move {
                        let mut hb_sys = System::new();
                        let mut interval = tokio::time::interval(std::time::Duration::from_secs(self.heartbeat_interval_seconds));
                        loop {
                            interval.tick().await;
                            hb_sys.refresh_cpu_all();
                            hb_sys.refresh_memory();
                            let disks = sysinfo::Disks::new_with_refreshed_list();
                            let disk_used: u64 = disks.iter()
                                .filter(|d| d.mount_point() == std::path::Path::new("/"))
                                .map(|d| d.total_space().saturating_sub(d.available_space()))
                                .sum();
                            let disk_load = if disk_total > 0 {
                                Some(disk_used as f64 / disk_total as f64)
                            } else {
                                None
                            };
                            let hb = AgentHeartbeatRequest {
                                status: AgentStatus::Online,
                                running_task_ids: vec![],
                                disk_free_bytes: None,
                                cpu_load: Some(hb_sys.global_cpu_usage() as f64 / 100.0),
                                memory_load: Some(hb_sys.used_memory() as f64 / memory_total as f64),
                                disk_load,
                                thread_num: Some(thread_count),
                            };
                            let mut tx = hb_tx.lock().await;
                            if send_message(&mut *tx, &InternalMessage::Heartbeat(hb)).await.is_err() {
                                break;
                            }
                        }
                    });

                    loop {
                        tokio::select! {
                            msg = self.msg_rx.recv() => {
                                match msg {
                                    Some(m) => {
                                        let mut tx = framed_tx.lock().await;
                                        if send_message(&mut *tx, &m).await.is_err() {
                                            break;
                                        }
                                    }
                                    None => break,
                                }
                            }
                            result = async {
                                let mut rx = framed_rx.lock().await;
                                recv_message(&mut *rx).await
                            } => {
                                match result {
                                    Ok(Some(msg)) => {
                                        if matches!(&msg, InternalMessage::HeartbeatAck) {
                                            continue;
                                        }
                                        if self.msg_tx.send(msg).await.is_err() {
                                            break;
                                        }
                                    }
                                    Ok(None) => break,
                                    Err(e) => {
                                        tracing::error!("Recv error: {e}");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Connect failed {addr}: {e}, retry in {}ms", retry_delay);
                    tokio::time::sleep(std::time::Duration::from_millis(retry_delay)).await;
                    retry_delay = (retry_delay * 2).min(self.reconnect_max_delay_ms);
                }
            }
        }
    }
}
