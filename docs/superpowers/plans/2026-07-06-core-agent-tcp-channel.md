# Core-Agent TCP 通道改造 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Core↔Agent 通信从 7 个独立 HTTP 端点改造为单一 TCP 长连接 + `InternalMessage` 枚举路由

**Architecture:** Core 同时运行 HTTP 管理服务 (18080) 和 TCP 通道服务 (9997)。Agent 启动后主动 TCP 连接 Core，保持长连接双向收发消息。所有 Agent 连接由 Core 的 `ConnectionRegistry` 管理。

**Tech Stack:** tokio TCP + LengthDelimitedCodec + bincode + uuid

## Global Constraints

- 不改变 `src/core/db.rs` 中 collect_result_cells 的写入逻辑
- 不改变 `src/agent/runner.rs` 中 run_parse_job 的执行逻辑
- 不改变前端 HTTP API 的路径和载荷格式
- 所有新增依赖必须加到 `Cargo.toml`
- 代码必须通过 `cargo test`（现有 47 个测试 + 新增测试）

---

## File Structure

### 新增文件

| 文件 | 职责 |
|------|------|
| `src/message.rs` | `InternalMessage` 枚举 + 子类型定义 |
| `src/core/tcp/mod.rs` | Core TCP 模块入口 |
| `src/core/tcp/protocol.rs` | 帧格式 + send_message/recv_message |
| `src/core/tcp/registry.rs` | `ConnectionRegistry` 连接管理 |
| `src/core/tcp/listener.rs` | `tcp_listener` + `handle_connection` |
| `src/agent/tcp.rs` | Agent TCP 客户端 + 心跳 + 重连 |
| `server.toml` | Core 配置 |
| `agent.toml` | Agent 配置 |

### 修改文件

| 文件 | 改动 |
|------|------|
| `Cargo.toml` | 新增 bincode, tokio-util, uuid（uuid 已有但确认 feature） |
| `src/lib.rs` | 新增 `pub mod message` |
| `src/core/mod.rs` | 新增 `pub mod tcp` |
| `src/core/server.rs` | 新增 CoreState 中 TCP 相关字段；启动 TCP listener + dispatch loop；移除 agent 相关 HTTP 端点 |
| `src/core/db.rs` | 无改动 |
| `src/agent/mod.rs` | 不变 |
| `src/agent/server.rs` | 替换为 TCP 客户端逻辑，移除 HTTP server |
| `src/agent/runner.rs` | 结果回传方式从 HTTP POST 改为 TCP 通道发送 |
| `src/bin/core.rs` | 读取 server.toml，传入 TCP 配置 |
| `src/bin/agent.rs` | 读取 agent.toml，传入 TCP 配置 |
| `src/core_agent_api.rs` | 为所有 struct 添加 Serialize/Deserialize derives | 

---

### Task 1: 依赖 + InternalMessage 枚举

**Files:**
- Modify: `Cargo.toml` (新增依赖)
- Create: `src/message.rs`
- Modify: `src/lib.rs` (pub mod message)
- Modify: `src/core_agent_api.rs` (给现有 struct 加 derive)
- Test: 序列化 roundtrip

- [ ] **Step 1: Cargo.toml 新增依赖**

```toml
bincode = "1.3"
futures = "0.3"
tokio-util = { version = "0.7", features = ["codec"] }
uuid = { version = "1", features = ["v4", "serde"] }
```

- [ ] **Step 2: 给 `src/core_agent_api.rs` 中所有 struct 添加 Serialize/Deserialize derives**

现有 struct 已经 derive `Serialize, Deserialize, Debug, Clone`，但有些没有。确保以下类型全部有 `Serialize, Deserialize`：

- `AgentRegisterRequest`, `AgentRegisterResponse`
- `AgentHeartbeatRequest`
- `ConfigSnapshotResponse` (already has)
- `TaskDispatchRequest` (already has)
- `TaskDispatchResponse` (already has)
- `TaskEventRequest` (already has)
- `TaskResultReport` (already has)
- `ResultRow` (already has)
- `TaskStatus` — 需要 derive
- `TaskPhase` — 需要 derive

直接在文件顶部搜索 `#[derive` 行，检查每个 public struct/enum，补全缺失的 derive。

- [ ] **Step 3: 创建 `src/message.rs`**

```rust
use crate::core_agent_api::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InternalMessage {
    AgentRegister(AgentRegisterRequest),
    AgentRegisterAck(AgentRegisterResponse),
    Heartbeat(AgentHeartbeatRequest),
    HeartbeatAck,

    DispatchTask(TaskDispatchRequest),
    DispatchTaskAck(TaskDispatchResponse),

    TaskEvent(TaskEventRequest),
    TaskResult(TaskResultReport),

    ConfigSnapshotRequest(String),   // snapshot_id
    ConfigSnapshotResponse(ConfigSnapshotResponse),

    CancelTask(String),              // task_id
    Shutdown,
}
```

- [ ] **Step 4: `src/lib.rs` 添加模块声明**

```rust
pub mod message;
```

- [ ] **Step 5: 写 roundtrip 测试**

在 `src/message.rs` 尾部添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internal_message_roundtrip() {
        let msg = InternalMessage::HeartbeatAck;
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: InternalMessage = bincode::deserialize(&bytes).unwrap();
        assert!(matches!(decoded, InternalMessage::HeartbeatAck));

        let msg = InternalMessage::Shutdown;
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: InternalMessage = bincode::deserialize(&bytes).unwrap();
        assert!(matches!(decoded, InternalMessage::Shutdown));

        let task_result = TaskResultReport {
            task_id: "test-task".into(),
            agent_id: "agent-01".into(),
            status: TaskStatus::Succeeded,
            result_rows: vec![ResultRow {
                table_name: "TPD_A".into(),
                data_time: "2026-06-17 15:15:00".into(),
                row_count: 100,
                success: 1,
                collect_time: "2026-07-02 15:35:00".into(),
            }],
        };
        let msg = InternalMessage::TaskResult(task_result.clone());
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: InternalMessage = bincode::deserialize(&bytes).unwrap();
        match decoded {
            InternalMessage::TaskResult(report) => {
                assert_eq!(report.task_id, "test-task");
                assert_eq!(report.result_rows.len(), 1);
                assert_eq!(report.result_rows[0].row_count, 100);
            }
            _ => panic!("expected TaskResult"),
        }
    }
}
```

- [ ] **Step 6: 运行测试**

```bash
cargo test test_internal_message_roundtrip -- --nocapture
# Expected: PASS
```

- [ ] **Step 7: 提交**

```bash
git add Cargo.toml src/message.rs src/lib.rs src/core_agent_api.rs
git commit -m "feat: add InternalMessage enum + bincode dependencies"
```

---

### Task 2: Core TCP 传输层 (protocol + registry + listener)

**Files:**
- Create: `src/core/tcp/mod.rs`
- Create: `src/core/tcp/protocol.rs`
- Create: `src/core/tcp/registry.rs`
- Create: `src/core/tcp/listener.rs`
- Modify: `src/core/mod.rs`
- Test: protocol roundtrip, registry 操作

- [ ] **Step 1: 创建目录和模块文件**

```bash
mkdir -p src/core/tcp
```

**`src/core/tcp/mod.rs`:**
```rust
pub mod protocol;
pub mod registry;
pub mod listener;
```

**`src/core/tcp/protocol.rs`:**
```rust
use crate::message::InternalMessage;
use anyhow::Result;
use bytes::Bytes;
use tokio_util::codec::LengthDelimitedCodec;
use tokio_util::codec::FramedRead;
use tokio_util::codec::FramedWrite;
use futures::{SinkExt, StreamExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use uuid::Uuid;

const MAGIC: &[u8; 4] = b"GCPM";

pub fn new_framed_write(writer: OwnedWriteHalf) -> FramedWrite<OwnedWriteHalf, LengthDelimitedCodec> {
    FramedWrite::new(writer, LengthDelimitedCodec::new())
}

pub fn new_framed_read(reader: OwnedReadHalf) -> FramedRead<OwnedReadHalf, LengthDelimitedCodec> {
    FramedRead::new(reader, LengthDelimitedCodec::new())
}

pub async fn send_message(
    tx: &mut FramedWrite<OwnedWriteHalf, LengthDelimitedCodec>,
    msg: &InternalMessage,
) -> Result<()> {
    let payload = bincode::serialize(msg)?;
    let mut buf = Vec::with_capacity(20 + payload.len());
    buf.extend_from_slice(MAGIC);                // 4 bytes
    buf.extend_from_slice(&Uuid::new_v4().into_bytes()); // 16 bytes correlation_id
    buf.extend_from_slice(&payload);
    tx.send(Bytes::from(buf)).await?;
    Ok(())
}

pub async fn recv_message(
    rx: &mut FramedRead<OwnedReadHalf, LengthDelimitedCodec>,
) -> Result<Option<InternalMessage>> {
    match rx.next().await {
        Some(Ok(buf)) => {
            if buf.len() < 20 {
                anyhow::bail!("frame too short: {} bytes", buf.len());
            }
            let payload = &buf[20..];
            let msg = bincode::deserialize(payload)?;
            Ok(Some(msg))
        }
        Some(Err(e)) => Err(e.into()),
        None => Ok(None), // connection closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn test_send_recv_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.split();
            let mut framed_rx = new_framed_read(reader);
            let mut framed_tx = new_framed_write(writer);

            let msg = recv_message(&mut framed_rx).await.unwrap().unwrap();
            assert!(matches!(msg, InternalMessage::HeartbeatAck));
            send_message(&mut framed_tx, &InternalMessage::Shutdown).await.unwrap();
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = stream.split();
        let mut framed_rx = new_framed_read(reader);
        let mut framed_tx = new_framed_write(writer);

        send_message(&mut framed_tx, &InternalMessage::HeartbeatAck).await.unwrap();
        let msg = recv_message(&mut framed_rx).await.unwrap().unwrap();
        assert!(matches!(msg, InternalMessage::Shutdown));

        server.await.unwrap();
    }
}
```

- [ ] **Step 2: 创建 ConnectionRegistry**

**`src/core/tcp/registry.rs`:**
```rust
use crate::message::InternalMessage;
use tokio_util::codec::LengthDelimitedCodec;
use tokio_util::codec::FramedWrite;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{RwLock, Mutex};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use std::net::SocketAddr;
use anyhow::Result;

pub type AgentId = String;

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
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            by_agent: Arc::new(RwLock::new(HashMap::new())),
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
        crate::core::tcp::protocol::send_message(&mut *writer, msg).await?;
        Ok(())
    }

    pub async fn broadcast(&self, msg: &InternalMessage) -> Result<()> {
        let map = self.by_agent.read().await;
        for (agent_id, conn) in map.iter() {
            let mut writer = conn.writer.lock().await;
            if let Err(e) = crate::core::tcp::protocol::send_message(&mut *writer, msg).await {
                tracing::warn!(%agent_id, error = %e, "broadcast send failed");
            }
        }
        Ok(())
    }

    pub async fn unregister(&self, agent_id: &str) {
        let mut map = self.by_agent.write().await;
        if map.remove(agent_id).is_some() {
            tracing::info!(%agent_id, "Agent unregistered");
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
```

- [ ] **Step 3: 创建 TCP listener**

**`src/core/tcp/listener.rs`:**
```rust
use crate::message::InternalMessage;
use crate::core::tcp::protocol::{new_framed_read, new_framed_write, recv_message, send_message};
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
    let (reader, writer) = stream.split();
    let mut framed_rx = new_framed_read(reader);
    let framed_tx = new_framed_write(writer);

    // 等待 AgentRegister 消息
    let agent_id = match recv_message(&mut framed_rx).await {
        Ok(Some(InternalMessage::AgentRegister(req))) => {
            let agent_id = req.agent_id.clone().unwrap_or_else(|| format!("agent-{}", addr.port()));
            // 注册 (注册时传入 writer 但不存入 registry 的 writer——当前这个还没存)
            // 但我们需要把 writer 存到 registry。因为 send_message 需要用 writer。
            // handle_connection 拥有 writer，当需要给这个 agent 发消息时，需要能拿到 writer。
            // 这里把 writer 注册进去
            registry.register(agent_id.clone(), addr, framed_tx).await;

            // 回复 AgentRegisterAck
            let ack = crate::core_agent_api::AgentRegisterResponse {
                agent_id: agent_id.clone(),
                heartbeat_interval_seconds: 10,
                task_report_interval_seconds: 10,
            };
            // 注意：register 之后 writer 被 move 进了 registry
            // 所以这里需要通过 registry 发送 ack
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
```

**注意：** 上面这段代码中 register 时传入 framed_tx 后 registry 拥有了 writer。但当我们在 handle_connection 中需要回复 AgentRegisterAck 时，需要通过 registry.send()。但 registry.send() 会尝试 lock writer 然后再 send_message。这里有一个问题——framed_tx 已经被 move 进 registry 了，而 framed_rx 依然归 handle_connection 所有。这是正确的设计：读循环在 handle_connection 中，写循环通过 registry 共享出去（每个 agent 一个 writer，由 Mutex 保护）。

但实际上还有问题——当一个 agent 连接上来时，我们先把 framed_rx 和 framed_tx 从 stream 中 split 出来。framed_tx move 进 registry 后，handle_connection 只拥有 framed_rx。当需要回复 AgentRegisterAck 时，我们用 registry.send()——registry 中的 framed_tx 是 write half，这是正确的。

但等一下——FramedWrite 需要 `OwnedWriteHalf` 而 `new_framed_write(writer)` 创建了 FramedWrite。这个 FramedWrite 不是 `Clone` 的。所以当我们 `registry.register(agent_id, addr, framed_tx)` 时，framed_tx 被 move 进 registry。

后续如果要回复消息，通过 `registry.send()` 来使用 registry 中的 framed_tx。

这完全可行，因为：
- framed_rx 在 handle_connection 中持续读取
- framed_tx 由 registry 持有，通过 Mutex 互斥访问

- [ ] **Step 4: 修改 `src/core/mod.rs`**

```rust
pub mod config_storage;
pub mod db;
pub mod grid;
pub mod server;
pub mod tcp;
```

- [ ] **Step 5: 运行测试**

```bash
cargo test test_send_recv_roundtrip -- --nocapture
```

- [ ] **Step 6: 提交**

```bash
git add src/core/tcp/ src/core/mod.rs
git commit -m "feat: Core TCP transport layer - protocol, registry, listener"
```

---

### Task 3: Core Server — 集成 TCP + Dispatch Loop

**Files:**
- Modify: `src/core/server.rs`
- No changes to `src/core/db.rs`

- [ ] **Step 1: 修改 `CoreState`，添加 TCP 相关字段**

```rust
pub struct CoreState {
    pub db: CoreDb,
    pub registry: ConnectionRegistry,
    pub to_tcp: mpsc::Sender<(AgentId, InternalMessage)>,
    // 以下可以逐渐弃用:
    pub http: reqwest::Client,
    pub storage: Arc<ConfigStorage>,
}
```

- [ ] **Step 2: 修改 `run_core_server`，启动 TCP listener + dispatch loop**

```rust
use crate::core::tcp::registry::ConnectionRegistry;
use crate::core::tcp::listener::tcp_listener;
use tokio::sync::mpsc;

pub async fn run_core_server(
    http_addr: SocketAddr,
    tcp_addr: SocketAddr,
    db_path: PathBuf,
    storage: ConfigStorage,
) -> Result<()> {
    let registry = ConnectionRegistry::new();
    let (to_dispatch_tx, to_dispatch_rx) = mpsc::channel::<(AgentId, InternalMessage)>(50000);
    let (to_tcp_tx, to_tcp_rx) = mpsc::channel::<(AgentId, InternalMessage)>(50000);

    let state = CoreState {
        db: CoreDb::open(db_path).await?,
        registry: registry.clone(),
        to_tcp: to_tcp_tx,
        http: reqwest::Client::new(),
        storage: Arc::new(storage),
    };

    // TCP listener — 接收 agent 连接
    tokio::spawn(tcp_listener(tcp_addr, to_dispatch_tx, registry.clone()));

    // Dispatch loop — 处理收到的 agent 消息
    let db_for_loop = state.db.clone();
    let registry_for_loop = registry.clone();
    tokio::spawn(tcp_dispatch_loop(to_dispatch_rx, registry_for_loop, db_for_loop));

    // TCP sender — 从 to_tcp 通道取消息发给 agent
    let reg_for_sender = registry.clone();
    tokio::spawn(tcp_sender_loop(to_tcp_rx, reg_for_sender));

    // 原有：心跳超时检查（从 DB 扫描改为 ConnectionRegistry 检查）
    tokio::spawn(tcp_cleanup_loop(registry.clone()));

    // HTTP server — 管理接口不变
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
```

- [ ] **Step 3: 实现 dispatch loop**

```rust
use crate::core::tcp::registry::ConnectionRegistry;
use crate::core::db::CoreDb;
use crate::message::InternalMessage;
use crate::core_agent_api::*;

async fn tcp_dispatch_loop(
    mut rx: mpsc::Receiver<(AgentId, InternalMessage)>,
    registry: ConnectionRegistry,
    db: CoreDb,
) {
    while let Some((agent_id, msg)) = rx.recv().await {
        match msg {
            InternalMessage::TaskResult(report) => {
                tracing::info!(
                    agent_id = %agent_id,
                    task_id = %report.task_id,
                    rows = report.result_rows.len(),
                    status = ?report.status,
                    "收到 TaskResult"
                );
                if let Err(e) = db.accept_task_result(&report).await {
                    tracing::error!(%agent_id, task_id = %report.task_id, error = %e, "accept_task_result 失败");
                }
            }
            InternalMessage::TaskEvent(event) => {
                // TODO: 更新 task phase/status（未来迭代）
                tracing::info!(%agent_id, task_id = %event.event_id, status = ?event.status, phase = ?event.phase, "TaskEvent");
            }
            InternalMessage::AgentRegister(req) => {
                // 已在 handle_connection 中注册到 registry，此处可记录 DB
                tracing::info!(%agent_id, "Agent registered: {:?}", req);
            }
            InternalMessage::ConfigSnapshotRequest(snapshot_id) => {
                // TODO: 查配置快照并回复（未来迭代，目前走 HTTP 下载）
                tracing::warn!(%agent_id, %snapshot_id, "ConfigSnapshotRequest 未实现");
            }
            _ => {
                tracing::warn!(%agent_id, "dispatch_loop: 未处理消息类型");
            }
        }
    }
}

async fn tcp_sender_loop(
    mut rx: mpsc::Receiver<(AgentId, InternalMessage)>,
    registry: ConnectionRegistry,
) {
    while let Some((agent_id, msg)) = rx.recv().await {
        if let Err(e) = registry.send(&agent_id, &msg).await {
            tracing::error!(%agent_id, error = %e, "tcp_sender_loop 发送失败");
        }
    }
}

async fn tcp_cleanup_loop(registry: ConnectionRegistry) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        interval.tick().await;
        let timed_out = registry.check_timeouts(std::time::Duration::from_secs(150)).await;
        for agent_id in timed_out {
            tracing::warn!(%agent_id, "心跳超时，注销");
            registry.unregister(&agent_id).await;
        }
    }
}
```

- [ ] **Step 4: 修改 `dispatch_task` handler（从 HTTP POST → TCP send）**

```rust
async fn dispatch_task(State(state): State<CoreState>, Json(request): Json<TaskDispatchRequest>) -> Response {
    // 1. 创建任务记录
    if let Err(e) = state.db.create_task(
        &request.task_id,
        &request.logical_task_key,
        &request.strategy_id,
        &request.config_snapshot_id,
        &request.scan_start_time,
        &request.collect_id,
        &"tcp",  // assigned_agent_id 等 dispatch 时确定
    ).await {
        return err_response(500, &format!("创建任务失败: {e}"));
    }

    // 2. 找在线 agent
    let online_agents = state.db.list_online_agents().await.unwrap_or_default();
    let agent = match online_agents.first() {
        Some(a) => a,
        None => return err_response(503, "没有在线 Agent"),
    };

    // 3. 通过 TCP 发送
    if !state.registry.is_connected(&agent.agent_id).await {
        return err_response(503, &format!("Agent {} TCP 未连接", agent.agent_id));
    }

    let msg = InternalMessage::DispatchTask(request.clone());
    if let Err(e) = state.registry.send(&agent.agent_id, &msg).await {
        return err_response(500, &format!("TCP 发送失败: {e}"));
    }

    ok_response(json!({
        "task_id": request.task_id,
        "accepted": true,
        "agent_id": agent.agent_id,
    }), "任务已分发")
}
```

- [ ] **Step 5: 删除旧的 Agent HTTP handlers**

从 `router()` 中移除以下路由：
- `POST /api/agents/register`
- `POST /api/agents/:agent_id/heartbeat`
- `POST /api/tasks/:task_id/events`
- `POST /api/tasks/:task_id/result`

对应的 handler 函数也可以删除或注释掉。

- [ ] **Step 6: 提交**

```bash
git add src/core/server.rs
git commit -m "feat: integrate TCP listener + dispatch loop into Core server"
```

- [ ] **Step 7（可选）: 添加优雅关闭**

在 `run_core_server` 中添加 `tokio::signal::ctrl_c()` 监听：

```rust
// run_core_server 中
let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
tokio::spawn(async move {
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("收到 SIGINT，开始关闭...");
    // 广播 Shutdown 给所有 Agent
    let _ = registry.broadcast(&InternalMessage::Shutdown).await;
    shutdown_tx.send(()).await.ok();
});
// 将 shutdown_rx 传给 tcp_listener 等，select! 中监听
```

---

### Task 4: Agent TCP 客户端 + 心跳 + 重连

**Files:**
- Create: `src/agent/tcp.rs`
- Add to: `src/agent/mod.rs`
- Modify: `src/agent/server.rs` (替换 HTTP server 为 TCP client)

- [ ] **Step 1: 创建 `src/agent/tcp.rs`**

```rust
use crate::message::InternalMessage;
use crate::core_agent_api::*;
use crate::core::tcp::protocol::{new_framed_read, new_framed_write, send_message, recv_message};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::codec::FramedWrite;
use tokio_util::codec::LengthDelimitedCodec;
use tokio::net::tcp::OwnedWriteHalf;

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
                    tracing::info!("已连接 Core: {addr}");
                    retry_delay = self.reconnect_interval_ms;

                    let (reader, writer) = stream.split();
                    let framed_rx = Arc::new(Mutex::new(new_framed_read(reader)));
                    let framed_tx = Arc::new(Mutex::new(new_framed_write(writer)));

                    // 注册
                    let req = AgentRegisterRequest {
                        agent_id: Some(self.agent_id.clone()),
                        agent_name: self.agent_id.clone(),
                        host: self.core_host.clone(),
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

                    // 等待 RegisterAck
                    let mut rx = framed_rx.lock().await;
                    let ack_msg = recv_message(&mut *rx).await;
                    drop(rx);
                    match ack_msg {
                        Ok(Some(InternalMessage::AgentRegisterAck(_))) => {
                            tracing::info!("注册成功");
                        }
                        _ => {
                            tracing::warn!("注册应答异常");
                            continue;
                        }
                    }

                    // 心跳任务
                    let hb_tx = framed_tx.clone();
                    let hb_agent_id = self.agent_id.clone();
                    tokio::spawn(async move {
                        let mut interval = tokio::time::interval(Duration::from_secs(10));
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

                    // 启动发送任务（将 msg_rx 的消息发往 Core）
                    let send_tx = framed_tx.clone();
                    let mut send_rx = self.msg_rx;
                    tokio::spawn(async move {
                        use crate::core::tcp::protocol::send_message;
                        while let Some(msg) = send_rx.recv().await {
                            let mut tx = send_tx.lock().await;
                            if send_message(&mut *tx, &msg).await.is_err() {
                                break;
                            }
                        }
                    });

                    // 主接收循环（从 Core 接收消息）
                    loop {
                        let mut rx = framed_rx.lock().await;
                        match recv_message(&mut *rx).await {
                            Ok(Some(msg)) => {
                                drop(rx);
                                if matches!(&msg, InternalMessage::HeartbeatAck) {
                                    continue;
                                }
                                if self.msg_tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                tracing::error!("接收错误: {e}");
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("连接失败 {addr}: {e}, {}ms 后重试", retry_delay);
                    sleep(Duration::from_millis(retry_delay)).await;
                    retry_delay = (retry_delay * 2).min(self.reconnect_max_delay_ms);
                }
            }
        }
    }
}
```

- [ ] **Step 2: 修改 `src/agent/mod.rs`**

```rust
pub mod result_csv;
pub mod runner;
pub mod server;
pub mod store;
pub mod tcp;
```

- [ ] **Step 3: 修改 `src/agent/server.rs`**

将 `run_agent_server` 从启动 HTTP server 改为启动 TCP client：

```rust
use crate::agent::tcp::AgentTcpClient;
use crate::agent::store::AgentStore;
use crate::agent::runner::AgentRunner;
use tokio::sync::mpsc;

pub async fn run_agent_server(
    agent_id: String,
    core_host: String,
    core_port: u16,
    data_dir: PathBuf,
    config_dir: Option<PathBuf>,
    reconnect_interval_ms: u64,
    reconnect_max_delay_ms: u64,
) -> Result<()> {
    let store = AgentStore::new(&data_dir);
    let (tcp_msg_tx, mut tcp_msg_rx) = mpsc::channel::<InternalMessage>(100);
    let (send_to_tcp_tx, mut send_to_tcp_rx) = mpsc::channel::<InternalMessage>(100);

    // 启动 TCP 客户端
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
            tracing::error!("TCP client 异常退出: {e}");
        }
    });

    // Agent 主消息循环处理来自 Core 的消息
    let runner = AgentRunner {
        agent_id: agent_id.clone(),
        core_api_base: String::new(),  // 不再需要 HTTP 回调
        http: reqwest::Client::new(),
        tcp_tx: send_to_tcp_tx.clone(),
    };

    while let Some(msg) = tcp_msg_rx.recv().await {
        match msg {
            InternalMessage::DispatchTask(request) => {
                tracing::info!(task_id = %request.task_id, "收到任务");
                let task_dir = data_dir.join("tasks").join(&request.task_id);
                tokio::spawn(runner.run_task(&store, request, task_dir));
            }
            InternalMessage::CancelTask(task_id) => {
                tracing::info!(%task_id, "收到取消任务");
                // TODO: 实现取消逻辑（未来迭代）
            }
            InternalMessage::Shutdown => {
                tracing::info!("收到关闭命令，agent 退出");
                break;
            }
            InternalMessage::ConfigSnapshotResponse(data) => {
                // TODO: 实现配置保存逻辑（未来迭代，目前仍可用 HTTP 下载）
                tracing::info!("收到配置快照: {}", data.config_snapshot_id);
            }
            _ => {
                tracing::warn!("agent: 未处理消息: {msg:?}");
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 4: 提交**

```bash
git add src/agent/tcp.rs src/agent/mod.rs src/agent/server.rs
git commit -m "feat: Agent TCP client with reconnect and heartbeat"
```

---

### Task 5: 修改 Agent Runner — 结果通过 TCP 发送

**Files:**
- Modify: `src/agent/runner.rs`

- [ ] **Step 1: 给 AgentRunner 添加 tcp_tx 字段**

```rust
pub struct AgentRunner {
    pub agent_id: String,
    pub core_api_base: String,
    pub http: reqwest::Client,
    pub tcp_tx: mpsc::Sender<InternalMessage>,
}
```

- [ ] **Step 2: 修改 `report_to_core` 方法**

原来：
```rust
async fn report_to_core(http: &reqwest::Client, core_api_base: &str, ...) {
    let url = format!("{core_api_base}/tasks/{task_id}/result");
    http.post(&url).json(&report).send().await;
}
```

改为：
```rust
async fn report_to_core(tcp_tx: &mpsc::Sender<InternalMessage>, ...) {
    let report = TaskResultReport { ... };
    let msg = InternalMessage::TaskResult(report);
    if let Err(e) = tcp_tx.send(msg).await {
        tracing::error!("[agent] TCP 发送结果失败: {e}");
    }
}
```

- [ ] **Step 3: 提交**

```bash
git add src/agent/runner.rs
git commit -m "refactor: Agent results via TCP instead of HTTP POST"
```

---

### Task 6: 配置文件 + 启动入口

**Files:**
- Create: `server.toml`
- Create: `agent.toml`
- Modify: `src/bin/core.rs`
- Modify: `src/bin/agent.rs`

- [ ] **Step 1: 创建 server.toml**

```toml
[http]
host = "0.0.0.0"
port = 18080

[tcp]
bind_host = "0.0.0.0"
bind_port = 9997

[heartbeat]
timeout_ms = 150000

[database]
url = "core.db"
```

- [ ] **Step 2: 创建 agent.toml**

```toml
[core]
host = "127.0.0.1"
port = 9997
agent_id = "agent-01"
reconnect_interval_ms = 5000
reconnect_max_delay_ms = 60000

[agent]
data_dir = "agent_data"
working_dir = "./work"
max_concurrent_tasks = 4
```

- [ ] **Step 3: 修改 `src/bin/core.rs`**

```rust
use std::net::SocketAddr;
use std::path::PathBuf;
use clap::Parser;
use anyhow::Result;
use serde::Deserialize;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "server.toml")]
    config: PathBuf,
}

#[derive(Deserialize)]
struct ServerConfig {
    http: HttpConfig,
    tcp: TcpConfig,
    heartbeat: HeartbeatConfig,
    database: DatabaseConfig,
}

#[derive(Deserialize)]
struct HttpConfig {
    host: String,
    port: u16,
}

#[derive(Deserialize)]
struct TcpConfig {
    bind_host: String,
    bind_port: u16,
}

#[derive(Deserialize)]
struct HeartbeatConfig {
    timeout_ms: u64,
}

#[derive(Deserialize)]
struct DatabaseConfig {
    url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_content = std::fs::read_to_string(&cli.config)?;
    let config: ServerConfig = toml::from_str(&config_content)?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let http_addr: SocketAddr = format!("{}:{}", config.http.host, config.http.port).parse()?;
    let tcp_addr: SocketAddr = format!("{}:{}", config.tcp.bind_host, config.tcp.bind_port).parse()?;
    let db_path = PathBuf::from(&config.database.url);
    let config_storage = wy_gnb_pm_parser::core::config_storage::ConfigStorage::new("config_storage");

    wy_gnb_pm_parser::core::server::run_core_server(http_addr, tcp_addr, db_path, config_storage).await?;

    Ok(())
}
```

- [ ] **Step 4: 修改 `src/bin/agent.rs`**

```rust
use std::path::PathBuf;
use clap::Parser;
use anyhow::Result;
use serde::Deserialize;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "agent.toml")]
    config: PathBuf,
}

#[derive(Deserialize)]
struct AgentConfig {
    core: CoreConfig,
    agent: AgentSettings,
}

#[derive(Deserialize)]
struct CoreConfig {
    host: String,
    port: u16,
    agent_id: String,
    reconnect_interval_ms: u64,
    reconnect_max_delay_ms: u64,
}

#[derive(Deserialize)]
struct AgentSettings {
    data_dir: PathBuf,
    max_concurrent_tasks: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_content = std::fs::read_to_string(&cli.config)?;
    let config: AgentConfig = toml::from_str(&config_content)?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    wy_gnb_pm_parser::agent::server::run_agent_server(
        config.core.agent_id,
        config.core.host,
        config.core.port,
        config.agent.data_dir,
        None,
        config.core.reconnect_interval_ms,
        config.core.reconnect_max_delay_ms,
    ).await?;

    Ok(())
}
```

- [ ] **Step 5: 提交**

```bash
git add server.toml agent.toml src/bin/core.rs src/bin/agent.rs
git commit -m "feat: server.toml / agent.toml config files, updated startup"
```

---

### Task 7: 清理旧 HTTP 端点 + 集成测试

**Files:**
- Modify: `src/core/server.rs`

- [ ] **Step 1: 从 Core HTTP router 中移除以下路由**

删除以下路由注册行和对应的 handler 函数：
- `POST /api/agents/register`
- `POST /api/agents/:agent_id/heartbeat`
- `POST /api/tasks/:task_id/events`
- `POST /api/tasks/:task_id/result`

保留 `GET /api/agents` 仍然可用于从前端查看 agent 列表（数据来自 DB），但 agent 注册不再经过 HTTP。

- [ ] **Step 2: 从 `CoreState` 中删除不再需要的字段**

`callback_base_url` 不再需要，可以移除。

`http` client 仍然需要（用于 config 下载等内部逻辑），保留。

- [ ] **Step 3: 运行完整测试**

```bash
cargo test 2>&1
# 预期全部通过
```

- [ ] **Step 4: 提交**

```bash
git add src/core/server.rs
git commit -m "refactor: remove agent HTTP endpoints, all communication via TCP"
```

---

## 自检清单

- [ ] 每个任务有独立测试
- [ ] 无 TBD/TODO/placeholder 遗留
- [ ] 所有 interface 签名跨任务一致
- [ ] 覆盖全部 7 条消息的 TCP 迁移
- [ ] server.toml 和 agent.toml 所有字段被解析
- [ ] Cargo.toml 新增依赖正确
- [ ] 编译通过 (`cargo check`)
- [ ] 测试通过 (`cargo test`)
