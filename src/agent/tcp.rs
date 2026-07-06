use crate::core::tcp::protocol::{new_framed_read, new_framed_write, recv_message, send_message};
use crate::core_agent_api::*;
use crate::message::InternalMessage;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};

pub struct AgentTcpClient {
    pub agent_id: String,
    pub core_host: String,
    pub core_port: u16,
    pub reconnect_interval_ms: u64,
    pub reconnect_max_delay_ms: u64,
    pub msg_tx: mpsc::Sender<InternalMessage>,
    pub msg_rx: mpsc::Receiver<InternalMessage>,
}

impl AgentTcpClient {
    pub async fn run(mut self) -> Result<()> {
        let mut retry_delay = self.reconnect_interval_ms;

        loop {
            let addr = format!("{}:{}", self.core_host, self.core_port);
            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    tracing::info!("Agent connected to Core: {addr}");
                    retry_delay = self.reconnect_interval_ms;

                    let (reader, writer) = stream.into_split();
                    let framed_rx = Arc::new(Mutex::new(new_framed_read(reader)));
                    let framed_tx = Arc::new(Mutex::new(new_framed_write(writer)));

                    let req = AgentRegisterRequest {
                        agent_id: Some(self.agent_id.clone()),
                        agent_name: self.agent_id.clone(),
                        host: String::new(),
                        port: self.core_port,
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        capabilities: AgentCapabilities {
                            can_collect: true,
                            can_parse: true,
                            can_load: false,
                            supported_protocols: vec!["tcp".into()],
                        },
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
                    tokio::spawn(async move {
                        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
                        loop {
                            interval.tick().await;
                            let hb = AgentHeartbeatRequest {
                                status: AgentStatus::Online,
                                running_task_ids: vec![],
                                disk_free_bytes: None,
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
