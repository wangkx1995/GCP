use crate::message::InternalMessage;
use tokio_util::codec::LengthDelimitedCodec;
use tokio_util::codec::FramedWrite;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{RwLock, Mutex};
use tokio::task::JoinHandle;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use std::net::SocketAddr;
use anyhow::Result;

pub type AgentId = String;

#[allow(dead_code)]
struct Connection {
    pub agent_id: AgentId,
    pub addr: SocketAddr,
    pub writer: Mutex<FramedWrite<OwnedWriteHalf, LengthDelimitedCodec>>,
    pub last_heartbeat: Instant,
    pub registered_at: Instant,
}

#[derive(Clone)]
pub struct ConnectionRegistry {
    by_agent: Arc<RwLock<HashMap<AgentId, Connection>>>,
    handles: Arc<tokio::sync::Mutex<HashMap<AgentId, JoinHandle<()>>>>,
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            by_agent: Arc::new(RwLock::new(HashMap::new())),
            handles: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub async fn register(
        &self,
        agent_id: AgentId,
        addr: SocketAddr,
        writer: FramedWrite<OwnedWriteHalf, LengthDelimitedCodec>,
    ) {
        let mut map = self.by_agent.write().await;
        map.insert(agent_id.clone(), Connection {
            agent_id: agent_id.clone(),
            addr,
            writer: Mutex::new(writer),
            last_heartbeat: Instant::now(),
            registered_at: Instant::now(),
        });
        tracing::info!(%agent_id, "Agent registered");
    }

    pub async fn send(&self, agent_id: &str, msg: &InternalMessage) -> Result<()> {
        let map = self.by_agent.read().await;
        let conn = map.get(agent_id).ok_or_else(|| anyhow::anyhow!("agent {agent_id} not connected"))?;
        let mut writer = conn.writer.lock().await;
        crate::core::tcp::protocol::send_message(&mut writer, msg).await?;
        Ok(())
    }

    pub async fn broadcast(&self, msg: &InternalMessage) -> Result<()> {
        let map = self.by_agent.read().await;
        for (agent_id, conn) in map.iter() {
            let mut writer = conn.writer.lock().await;
            if let Err(e) = crate::core::tcp::protocol::send_message(&mut writer, msg).await {
                tracing::warn!(%agent_id, error = %e, "broadcast send failed");
            }
        }
        Ok(())
    }

    pub async fn store_handle(&self, agent_id: &str, handle: JoinHandle<()>) {
        let mut map = self.handles.lock().await;
        if let Some(old) = map.remove(agent_id) {
            old.abort();
            tracing::info!(%agent_id, "aborted stale connection task");
        }
        map.insert(agent_id.to_string(), handle);
    }

    pub async fn unregister(&self, agent_id: &str) {
        let mut map = self.by_agent.write().await;
        if map.remove(agent_id).is_some() {
            tracing::info!(%agent_id, "Agent unregistered");
        }
        let mut handle_map = self.handles.lock().await;
        if let Some(handle) = handle_map.remove(agent_id) {
            handle.abort();
        }
    }

    pub async fn update_heartbeat(&self, agent_id: &str) {
        let mut map = self.by_agent.write().await;
        if let Some(conn) = map.get_mut(agent_id) {
            conn.last_heartbeat = Instant::now();
        }
    }

    pub async fn check_timeouts(&self, timeout: std::time::Duration) -> Vec<AgentId> {
        let mut timed_out = Vec::new();
        let map = self.by_agent.read().await;
        for (agent_id, conn) in map.iter() {
            if conn.last_heartbeat.elapsed() > timeout {
                timed_out.push(agent_id.clone());
            }
        }
        timed_out
    }

    pub async fn is_connected(&self, agent_id: &str) -> bool {
        self.by_agent.read().await.contains_key(agent_id)
    }

    pub async fn online_count(&self) -> usize {
        self.by_agent.read().await.len()
    }

    pub async fn online_agents(&self) -> Vec<AgentId> {
        self.by_agent.read().await.keys().cloned().collect()
    }
}
