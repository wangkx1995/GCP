# Core-Agent TCP 长连接通道改造设计

> 基于 `docs/superpowers/specs/2026-07-02-core-agent-collection-design.md` 的 HTTP REST 原型，将 Core↔Agent 通信从多端点 HTTP 改造为单一 TCP 长连接 + `InternalMessage` 枚举路由。

---

## 一、架构概览

```
浏览器 ──HTTP──▶ Core (REST API) ──TCP──▶ Agent
                port 18080         port 9997
                                      ◀──TCP──
```

- **Core** 同时运行 HTTP 管理服务（18080）和 TCP 通道服务（9997）
- **Agent** 启动后主动 TCP 连 Core，保持长连接，双向收发
- 前端 HTTP API 完全不变

---

## 二、配置文件

### server.toml（Core 启动配置）

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
url = "sqlite:core.db"
max_connections = 10
```

### agent.toml（Agent 启动参数）

```toml
[core]
host = "127.0.0.1"
port = 9997
agent_id = "agent-01"
reconnect_interval_ms = 5000
reconnect_max_delay_ms = 60000

[agent]
working_dir = "./work"
max_concurrent_tasks = 4
```

---

## 三、消息协议（InternalMessage）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InternalMessage {
    // ── 注册与心跳 ──
    AgentRegister(AgentRegisterReq),
    AgentRegisterAck(AgentRegisterResp),
    Heartbeat(HeartbeatData),
    HeartbeatAck,

    // ── 任务派发 ──
    DispatchTask(TaskDispatchRequest),
    DispatchTaskAck(TaskDispatchAck),

    // ── 任务生命周期 ──
    TaskEvent(TaskEventRequest),
    TaskResult(TaskResultReport),

    // ── 配置快照 ──
    ConfigSnapshotRequest(ConfigSnapshotId),
    ConfigSnapshotResponse(ConfigSnapshotData),

    // ── 控制 ──
    CancelTask(TaskId),
    Shutdown,
}
```

所有消息类型定义集中在 `src/message.rs`，Core 和 Agent 共享。

---

## 四、TCP 传输协议

### 4.1 帧格式

使用 `tokio_util::codec::LengthDelimitedCodec` + bincode：

```
┌──────────────────────────────────────┐
│  4 bytes: frame_length (big-endian)  │  ← LengthDelimitedCodec
├──────────────────────────────────────┤
│  4 bytes: magic 0x4743504D ("GCPM")  │
│ 16 bytes: correlation_id (UUID)      │  可选，用于 request-response
│  n bytes: payload (bincode encoded)  │  InternalMessage
└──────────────────────────────────────┘
```

### 4.2 序列化

```rust
use tokio_util::codec::{LengthDelimitedCodec, FramedRead, FramedWrite};
use bytes::Bytes;

async fn send_message(
    tx: &mut FramedWrite<OwnedWriteHalf, LengthDelimitedCodec>,
    msg: &InternalMessage,
) -> Result<()> {
    let payload = bincode::serialize(msg)?;
    // 前置 magic + correlation_id
    let mut buf = Vec::with_capacity(20 + payload.len());
    buf.extend_from_slice(b"GCPM");
    buf.extend_from_slice(&Uuid::new_v4().into_bytes());
    buf.extend_from_slice(&payload);
    tx.send(Bytes::from(buf)).await?;
    Ok(())
}

async fn recv_message(
    rx: &mut FramedRead<OwnedReadHalf, LengthDelimitedCodec>,
) -> Result<InternalMessage> {
    let buf = rx.next().await.ok_or(Error::ConnectionClosed)??;
    let payload = &buf[20..]; // skip magic(4) + correlation_id(16)
    let msg = bincode::deserialize(payload)?;
    Ok(msg)
}
```

---

## 五、Core 端架构

### 5.1 TCP 服务端

```rust
pub async fn tcp_listener(
    cfg: &TcpConfig,
    to_dispatch: mpsc::Sender<(AgentId, InternalMessage)>,
    registry: Arc<ConnectionRegistry>,
) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", cfg.bind_host, cfg.bind_port)).await?;
    loop {
        let (stream, addr) = listener.accept().await?;
        let to_dispatch = to_dispatch.clone();
        let registry = registry.clone();
        tokio::spawn(handle_connection(addr, stream, to_dispatch, registry));
    }
}
```

### 5.2 连接注册表（ConnectionRegistry）

```rust
struct Connection {
    agent_id: AgentId,
    addr: SocketAddr,
    writer: Arc<Mutex<FramedWrite<OwnedWriteHalf, LengthDelimitedCodec>>>,
    last_heartbeat: Instant,
}

struct ConnectionRegistry {
    by_agent: Arc<RwLock<HashMap<AgentId, Connection>>>,
    by_addr:  Arc<RwLock<HashMap<SocketAddr, AgentId>>>,
}

impl ConnectionRegistry {
    async fn register(agent_id, addr, writer) -> Result<()>;
    async fn send(agent_id, msg: InternalMessage) -> Result<()>;
    async fn broadcast(msg: InternalMessage) -> Result<()>;
    async fn check_timeouts(timeout: Duration) -> Vec<AgentId>;
    async fn unregister(agent_id);
}
```

### 5.3 Dispatch Loop

```rust
async fn dispatch_loop(
    mut from_tcp: mpsc::Receiver<(AgentId, InternalMessage)>,
    registry: Arc<ConnectionRegistry>,
    db: Database,
    to_tcp: mpsc::Sender<(AgentId, InternalMessage)>,
) {
    while let Some((agent_id, msg)) = from_tcp.recv().await {
        match msg {
            InternalMessage::AgentRegister(req) => { /* ... */ }
            InternalMessage::Heartbeat(hb) => { /* ... */ }
            InternalMessage::TaskResult(report) => {
                // 写入 collect_result_cells
                // 更新 task 为 SUCCEEDED
            }
            InternalMessage::TaskEvent(event) => {
                // 更新 task phase/status
            }
            InternalMessage::ConfigSnapshotRequest(id) => {
                // 查 DB 获取配置数据
                // to_tcp.send((agent_id, ConfigSnapshotResponse(data)))
            }
            _ => tracing::warn!("未处理消息: {:?}", msg),
        }
    }
}
```

### 5.4 Core 消息流

```
TCP 连接建立
  → handle_connection 解析帧
  → push (AgentId, InternalMessage) → from_tcp_tx
  → dispatch_loop 收到 → match → 处理
  → 如需回复 → push (AgentId, msg) → to_tcp_tx
  → tcp_sender 从 to_tcp_rx 接收 → 查 registry → writer.send(msg)
```

---

## 六、Agent 端架构

### 6.1 TCP 客户端

```rust
pub async fn run(cfg: &AgentConfig) -> Result<()> {
    loop {
        match TcpStream::connect(format!("{}:{}", cfg.core_host, cfg.core_port)).await {
            Ok(stream) => {
                let (reader, writer) = stream.split();
                let mut framed_rx = FramedRead::new(reader, LengthDelimitedCodec::new());
                let framed_tx = Arc::new(Mutex::new(FramedWrite::new(writer, LengthDelimitedCodec::new())));

                // 1. 注册
                send_register(&framed_tx, &cfg).await?;

                // 2. 启动心跳
                spawn_heartbeat(framed_tx.clone(), cfg.heartbeat_interval);

                // 3. 主接收循环
                while let Some(frame) = framed_rx.next().await {
                    let buf = frame?;
                    let msg: InternalMessage = bincode::deserialize(&buf[20..])?;
                    handle_incoming(msg).await?;
                }
            }
            Err(e) => {
                tracing::error!("连接失败: {}, {}s 后重试", e, retry_secs);
                sleep(Duration::from_secs(retry_secs)).await;
            }
        }
    }
}
```

### 6.2 Agent 消息处理

```rust
async fn handle_incoming(msg: InternalMessage) -> Result<()> {
    match msg {
        InternalMessage::DispatchTask(request) => {
            // 启动解析任务 (同当前 agent/runner.rs)
            tokio::spawn(run_task(request));
        }
        InternalMessage::CancelTask(task_id) => {
            // 取消指定任务
        }
        InternalMessage::ConfigSnapshotResponse(data) => {
            // 保存配置到本地
        }
        InternalMessage::Shutdown => {
            // 优雅关闭
        }
        InternalMessage::RegisterAck(ack) => {
            // 确认注册成功
        }
        _ => {}
    }
}
```

### 6.3 Agent 结果回传

解析任务完成后，不再 POST HTTP，而是通过 TCP 发送：

```rust
// agent/runner.rs 中
let report = TaskResultReport { task_id, agent_id, status, result_rows };
let msg = InternalMessage::TaskResult(report);
framed_tx.lock().await.send_message(&msg).await?;
```

---

## 七、优雅关闭

```
Core 关闭:
  1. 停止 TcpListener (停止接受新连接)
  2. 广播 Shutdown 给所有 Agent
  3. 等待 in-flight 消息处理完成
  4. 关闭 dispatch_loop
  5. 关闭数据库连接

Agent 关闭:
  1. 停止接收新消息
  2. 等待运行中的任务完成 (或超时取消)
  3. 发送最后的状态后关闭连接
```

使用 `tokio::signal::ctrl_c()` 捕获 SIGINT/SIGTERM。

---

## 八、文件改动清单

| 操作 | 文件 | 说明 |
|------|------|------|
| 新建 | `src/message.rs` | `InternalMessage` 枚举 + 子类型定义 |
| 新建 | `src/core/tcp/mod.rs` | Core TCP 模块 |
| 新建 | `src/core/tcp/listener.rs` | TCP 服务端 |
| 新建 | `src/core/tcp/registry.rs` | 连接注册表 |
| 新建 | `src/core/tcp/protocol.rs` | 帧格式 + 序列化 |
| 新增 | `src/core/config.rs` | server.toml / agent.toml 解析 |
| 修改 | `src/core/server.rs` | 移除 Agent 相关 HTTP 端点，启动 TCP listener |
| 修改 | `src/core/db.rs` | 无变动（仍用 sqlx 操作 collect_result_cells） |
| 修改 | `src/core_agent_api.rs` | 保留数据结构，统一 derive Serialize/Deserialize |
| 修改 | `src/agent/server.rs` | 删除 HTTP server，改为 TCP 客户端 |
| 新建 | `src/agent/tcp.rs` | Agent TCP 连接 + 心跳 + 重连 |
| 修改 | `src/agent/runner.rs` | 结果回传改为 TCP 发送 |
| 修改 | `src/bin/core.rs` | 读取 server.toml |
| 修改 | `src/bin/agent.rs` | 读取 agent.toml |
| 新建 | `server.toml` | Core 配置文件 |
| 新建 | `agent.toml` | Agent 配置文件 |

---

## 九、关键决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 序列化格式 | bincode | 二进制紧凑，Rust 原生支持，无需跨语言 |
| 帧编码 | LengthDelimitedCodec | tokio-util 内置，零额外依赖 |
| 连接方向 | Agent 连 Core | 参考 Java 版 QueueAcceptor 模式 |
| 注册时机 | 连接建立后立即注册 | Core 需在发消息前知道 Agent 身份 |
| 心跳 | Agent 定时发送，Core 检测超时 | ConnectionRegistry.check_timeouts() |
| 断线重连 | exponential backoff | 起始 5s，最大 60s |
| 配置格式 | TOML | 与现有 source.toml 风格一致 |

---

## 十、测试策略

1. **单元测试**：`ConnectionRegistry` 的注册/注销/超时
2. **单元测试**：`InternalMessage` 的 bincode 序列化/反序列化 roundtrip
3. **集成测试**：建立 TCP 连接，发送 `DispatchTask`，验证 Agent 回复
4. **集成测试**：断线重连场景
5. **冒烟测试**：完整周期（Agent 注册 → 派发任务 → 回传结果 → 网格可查）
