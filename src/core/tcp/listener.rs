use crate::message::InternalMessage;
use crate::core::tcp::protocol::{new_framed_read, new_framed_write, recv_message};
use crate::core::tcp::registry::{AgentId, ConnectionRegistry};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use std::net::SocketAddr;
use anyhow::Result;

pub async fn tcp_listener(
    bind_addr: SocketAddr,
    to_dispatch: mpsc::Sender<(AgentId, InternalMessage)>,
    registry: ConnectionRegistry,
) -> Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!("Core TCP listener started on {bind_addr}");

    loop {
        let (stream, addr) = listener.accept().await?;
        let to_dispatch = to_dispatch.clone();
        let registry = registry.clone();
        tokio::spawn(handle_connection(addr, stream, to_dispatch, registry));
    }
}

async fn handle_connection(
    addr: SocketAddr,
    stream: tokio::net::TcpStream,
    to_dispatch: mpsc::Sender<(AgentId, InternalMessage)>,
    registry: ConnectionRegistry,
) {
    let (reader, writer) = stream.into_split();
    let mut framed_rx = new_framed_read(reader);
    let framed_tx = new_framed_write(writer);

    // 等待 AgentRegister 消息
    let agent_id = match recv_message(&mut framed_rx).await {
        Ok(Some(InternalMessage::AgentRegister(req))) => {
            let agent_id = req.agent_id.clone().unwrap_or_else(|| format!("agent-{}", addr.port()));
            // 注册 (把 framed_tx move 进 registry)
            registry.register(agent_id.clone(), addr, framed_tx).await;

            // 回复 AgentRegisterAck (通过 registry.send)
            let ack = crate::core_agent_api::AgentRegisterResponse {
                agent_id: agent_id.clone(),
                heartbeat_interval_seconds: 10,
                task_report_interval_seconds: 10,
            };
            if let Err(e) = registry.send(&agent_id, &InternalMessage::AgentRegisterAck(ack)).await {
                tracing::error!(%addr, error = %e, "发送 register ack 失败");
                registry.unregister(&agent_id).await;
                return;
            }

            // 通知 dispatch loop 有新 agent 注册
            let _ = to_dispatch.send((agent_id.clone(), InternalMessage::AgentRegister(req))).await;

            agent_id
        }
        Ok(Some(other)) => {
            tracing::warn!(%addr, "期望 AgentRegister, 收到: {other:?}");
            return;
        }
        Ok(None) => {
            tracing::info!(%addr, "连接关闭(注册前)");
            return;
        }
        Err(e) => {
            tracing::error!(%addr, error = %e, "注册消息解析失败");
            return;
        }
    };

    // 主消息循环
    loop {
        match recv_message(&mut framed_rx).await {
            Ok(Some(msg)) => {
                match &msg {
                    InternalMessage::Heartbeat(_) => {
                        registry.update_heartbeat(&agent_id).await;
                        let _ = registry.send(&agent_id, &InternalMessage::HeartbeatAck).await;
                    }
                    // 其他消息转发给 dispatch loop
                    _ => {
                        if to_dispatch.send((agent_id.clone(), msg)).await.is_err() {
                            break;
                        }
                    }
                }
            }
            Ok(None) => break,  // 连接关闭
            Err(e) => {
                tracing::warn!(%agent_id, error = %e, "消息接收错误");
                break;
            }
        }
    }

    registry.unregister(&agent_id).await;
}

/// 通过 registry 给 agent 发送消息（dispatch loop 或其他模块调用）
pub async fn send_to_agent(registry: &ConnectionRegistry, agent_id: &str, msg: &InternalMessage) -> Result<()> {
    registry.send(agent_id, msg).await
}
