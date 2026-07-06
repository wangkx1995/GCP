# GCP 通道系统 Rust 重构方案

> 前提：完整重构 Core + Collector，两套进程统一设计。

---

## 一、核心理念：放弃"MQ"概念，回归"Channel as Glue"

Java 版需要自研 MQ 是因为 Java 缺少：
- 类型安全的消息路由（需反射 + datachannel.xml + Groovy）
- 轻量级异步通道（ArrayBlockingQueue 是阻塞的）
- 跨进程序列化（Java 原生序列化性能差）

**Rust 中这些已是语言/生态一等公民，MQ 层可完全溶解到代码中。**

```
Java:  ModuleA → [自研 NativeChannel] → DataDispatcher → [自研 NativeChannel] → ModuleB
                           ↓                                      ↓
                     [QueueAcceptor][DataStore][MessageQueue] → TCP

Rust:  ModuleA → tokio::mpsc → ModuleB        ← 同进程
       ModuleA → tokio::mpsc → TCPSender → TCP → TCPReceiver → tokio::mpsc → ModuleB
```

---

## 二、消息类型设计（替代 datachannel.xml + Rule）

```rust
/// 全系统唯一消息枚举 — 覆盖 Core↔Collector 间全部通信场景
/// 取代：Java 的 Rule + condition + datachannel.xml + Groovy Shell
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InternalMessage {
    // ── 策略与任务 ──
    OrderPolicy(OrderPolicy),
    SpecialTask(SpecialTask),
    PolicyAck(PolicyAck),
    TaskGroup(TaskGroup),
    SubTask(SubTask),
    TaskStatus(TaskStatus),

    // ── 采集机管理 ──
    Heartbeat(MdInfo),
    SystemStatus(SystemStatus),
    AdapterRegister(AdapterInfo),
    AdapterAck(AdapterAck),
    ConnectionStatus(ConnectionStatus),

    // ── 文件与数据 ──
    FileReady(FileReadyNotify),
    NeIntegrality(NeIntegrality),
    NeProcDesc(NeProcDesc),
    NeVendorData(NeVendorData),

    // ── 告警 ──
    AlarmDataQuality(AlarmDataQuality),
    NorthSynAlarm(NorthSynAlarm),

    // ── 控制与同步 ──
    Metadata(String),
    Sync(SyncCommand),
    LoadResult(LoadResult),

    // ── 通用 ──
    Text(String),
}
```

**效果对比：**

| Java | Rust |
|---|---|
| `datachannel.xml` 375 行 XML | 1 个 enum 定义 |
| `Rule.name` 正则匹配 VO 类名 | `match` 分支，编译期检查 |
| `Rule.condition` Groovy 运行时求值 | `match` guard，编译期检查 |
| `DataDispatcher.dispatch()` 反射路由 | `tx.send(msg)` 直接调用 |
| 发错 VO 类型？运行时 WARN 日志 | 发错类型？编译报错 |

### 条件路由对比

Java（datachannel.xml）：
```xml
<Rule name="TaskGroupVO" condition='DATA.isFlow()==false'>
    <Channel>NATIVE://TASKGROUP_TO_SPLIT</Channel>
</Rule>
<Rule name="TaskGroupVO" condition='DATA.isFlow()==true'>
    <Channel>NATIVE://TASKGROUP_TO_SPLIT_FLOW</Channel>
</Rule>
```

Rust（编译期匹配）：
```rust
match msg {
    InternalMessage::TaskGroup(tg) if !tg.is_flow() => {
        to_split.send(msg).await;
    }
    InternalMessage::TaskGroup(tg) if tg.is_flow() => {
        to_split_flow.send(msg).await;
    }
    _ => {} // 不匹配静默丢弃（等同 Java WARN 日志）
}
```

---

## 三、通道拓扑（取代 datachannel.xml 的 Module 连线）

### 3.1 通道定义

每个 Module 就是一段 async 函数，接收上游 `Receiver`，持有下游 `Sender`：

```rust
type ChanRx = mpsc::Receiver<InternalMessage>;
type ChanTx = mpsc::Sender<InternalMessage>;
```

### 3.2 模块映射表

| Java Module | receiveChannel | → | Rust 任务 | 上游 | 下游 |
|---|---|---|---|---|---|
| PolicyReceiveThread | GCP_POLICY_Q | → | `policy_receiver` | TCP 入站 | auth |
| PolicyCMDReceiveThread | GCP_CMD_Q | → | `cmd_receiver` | TCP 入站 | auth |
| AuthenticationThread | POLICY_AUTHENTICATION | → | `auth` | policy_receiver/cmd_receiver | classifier |
| PolicyClassifyThread | POLICY_CLASSIFY | → | `classifier` | auth | checker |
| CheckSinglePolicyThread | POLICY_CHECK_SINGLE | → | `checker` | classifier | task_group_creator |
| TaskGroupCreatorThread | POLICY_TO_TASKGROUP | → | `task_group_creator` | checker | task_uniter |
| TaskUniteThread | UNITE_TASK_QUEUE | → | `task_uniter` | task_group_creator | task_dispatcher |
| TaskDispatchThread | DISPATCH_POLICY_QUEUE | → | `task_dispatcher` | task_uniter | TCP 出站 |
| SocketMsgRcvrThread | (TCP) | → | `tcp_receiver` | TCP Socket | 按类型分发 |
| SocketMsgSndrThread | SOCKET_MSG_SENDER | → | `tcp_sender` | 各模块 | TCP Socket |
| StatusReciever | GCP_RESPONSE_Q | → | `status_receiver` | tcp_receiver | DB |
| AdapterManager | GCP_NOTICE_ADAPTER | → | `adapter_manager` | tcp_receiver | DB |
| CheckMDHeartBeatThread | (DB 扫描) | → | `heartbeat_monitor` | DB 定时扫描 | (告警/重分发) |
| FileReadyNotifyService | GCP_FILE_NT_Q | → | `file_notify` | tcp_receiver | north |

### 3.3 通道连接（启动时构建）

```rust
pub fn build_pipeline(cfg: &Config) -> Pipeline {
    // 1. 创建全量通道
    let (socket_rx_tx, mut socket_rx_rx) = mpsc::channel(50000);
    let (auth_tx, auth_rx) = mpsc::channel(50000);
    let (classify_tx, classify_rx) = mpsc::channel(50000);
    let (check_tx, check_rx) = mpsc::channel(50000);
    let (taskgroup_tx, taskgroup_rx) = mpsc::channel(50000);
    let (unite_tx, unite_rx) = mpsc::channel(50000);
    let (dispatch_tx, dispatch_rx) = mpsc::channel(50000);
    let (socket_tx_tx, socket_tx_rx) = mpsc::channel(50000);
    // ...

    // 2. 返回 Pipeline 结构体，所有任务共享通道
    Pipeline { socket_rx_tx, auth_tx, classify_tx, /* ... */ }
}
```

### 3.4 启动全模块

```rust
pub async fn run(pipeline: Pipeline) {
    // 每个模块是一个独立的 tokio 任务
    tokio::join!(
        tcp_receiver::run(pipeline.socket_rx_rx, pipeline.auth_tx.clone(), /* ... */),
        auth::run(pipeline.auth_rx, pipeline.classify_tx),
        classifier::run(pipeline.classify_rx, pipeline.check_tx),
        checker::run(pipeline.check_rx, pipeline.taskgroup_tx),
        task_group_creator::run(pipeline.taskgroup_rx, pipeline.unite_tx),
        task_uniter::run(pipeline.unite_rx, pipeline.dispatch_tx),
        task_dispatcher::run(pipeline.dispatch_rx, pipeline.socket_tx_tx),
        tcp_sender::run(pipeline.socket_tx_rx, cfg.peer_addr),
        heartbeat_monitor::run(cfg),
    );
}
```

---

## 四、跨进程 TCP 协议（取代 QueueAcceptor/DataStore/MessageQueue）

### 4.1 协议格式

```
┌──────────────────────────────────────────────┐
│    4 bytes: frame_length (big-endian, u32)    │
├──────────────────────────────────────────────┤
│    4 bytes: magic           (0x4743504D "GCPM")  │
│   16 bytes: correlation_id  (UUID, optional)  │
│    n bytes: payload         (bincode encoded) │
└──────────────────────────────────────────────┘
```

使用 tokio 的 `LengthDelimitedCodec` + bincode：

```rust
use tokio_util::codec::{LengthDelimitedCodec, FramedRead, FramedWrite};
use futures::{SinkExt, StreamExt};

// 发送
async fn send_message(tx: &mut FramedWrite<OwnedWriteHalf, LengthDelimitedCodec>, msg: InternalMessage) {
    let bytes = bincode::serialize(&msg)?;
    tx.send(Bytes::from(bytes)).await?;
}

// 接收
async fn recv_message(rx: &mut FramedRead<OwnedReadHalf, LengthDelimitedCodec>) -> Result<InternalMessage> {
    let buf = rx.next().await.unwrap()?;
    let msg = bincode::deserialize(&buf)?;
    Ok(msg)
}
```

### 4.2 服务端（取代 QueueAcceptor）

```rust
pub async fn tcp_listener(cfg: &Config, to_dispatcher: ChanTx) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", cfg.host, cfg.port)).await?;

    let mut next_id = 0u64;
    loop {
        let (stream, addr) = listener.accept().await?;
        let tx = to_dispatcher.clone();
        let conn_id = next_id;
        next_id += 1;

        tracing::info!(conn_id, %addr, "新连接");
        tokio::spawn(handle_connection(conn_id, addr, stream, tx));
    }
}

async fn handle_connection(conn_id: u64, addr: SocketAddr, stream: TcpStream, to_dispatcher: ChanTx) {
    let (reader, writer) = stream.split();
    let mut framed = FramedRead::new(reader, LengthDelimitedCodec::new());

    // 注册连接（用于心跳监控）
    CONNECTION_REGISTRY.register(conn_id, addr, writer).await;

    while let Some(frame) = framed.next().await {
        match frame {
            Ok(buf) => {
                match bincode::deserialize::<InternalMessage>(&buf) {
                    Ok(msg) => {
                        if to_dispatcher.send(msg).await.is_err() {
                            break; // 接收端已关闭
                        }
                    }
                    Err(e) => tracing::warn!(%conn_id, error = %e, "反序列化失败"),
                }
            }
            Err(e) => {
                tracing::warn!(%conn_id, error = %e, "读取错误");
                break;
            }
        }
    }

    CONNECTION_REGISTRY.unregister(conn_id).await;
    tracing::info!(%conn_id, "连接断开");
}
```

### 4.3 连接注册表（取代 DataStore）

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

struct Connection {
    id: u64,
    addr: SocketAddr,
    writer: FramedWrite<OwnedWriteHalf, LengthDelimitedCodec>,
    last_heartbeat: Instant,
}

struct ConnectionRegistry {
    connections: Arc<RwLock<HashMap<u64, Connection>>>,
}

impl ConnectionRegistry {
    async fn send_to(&self, conn_id: u64, msg: InternalMessage) -> Result<()> {
        let conn = self.connections.read().await;
        let entry = conn.get(&conn_id).ok_or(Error::ConnNotFound)?;
        let bytes = bincode::serialize(&msg)?;
        // clone writer 或使用内部 Arc
        // 实际使用 split 后的 writer 需要 Send
        todo!()
    }

    async fn broadcast(&self, msg: InternalMessage) -> Result<()> {
        let bytes = bincode::serialize(&msg)?;
        let conn = self.connections.read().await;
        for (id, conn) in conn.iter() {
            // 向所有 Collector 连接发送
        }
        Ok(())
    }

    /// 心跳超时检查（取代 CheckMDHeartBeatThread）
    async fn check_timeouts(&self, timeout: Duration) {
        let mut to_remove = Vec::new();
        {
            let conn = self.connections.read().await;
            for (id, conn) in conn.iter() {
                if conn.last_heartbeat.elapsed() > timeout {
                    to_remove.push(*id);
                }
            }
        }
        for id in to_remove {
            tracing::warn!(conn_id = %id, "连接超时");
            self.connections.write().await.remove(&id);
        }
    }
}
```

### 4.4 连接 vs DataStore 对比

| Java 概念 | Rust 等价物 |
|---|---|
| `QueueAcceptor` (Thread + ServerSocket) | `tokio::TcpListener` + `tokio::spawn` |
| `DataStore` (Map<String, NativeChannel>) | `ConnectionRegistry` (RwLock<HashMap>) |
| `NativeChannel<DataContainer>` (synchronized LinkedList, 容量 30000) | `mpsc::channel` (有界, 按队列配置) |
| `MessageQueue.write()` / `.read()` | `FramedWrite.send()` / `FramedRead.next()` |
| `QueueRequest/QueueResponse` (correlationId 模式) | 可选：`oneshot::channel` 实现 request-response |

---

## 五、模块实现模式（取代 dispose() + DataProcessThread）

### 5.1 标准模块模板

Java 每个 Module 继承 `DataProcessThread`，覆写 `dispose()`：

```java
class AuthenticationThread extends DataProcessThread {
    void dispose() {
        doHeartBeat(getClass());
        Object msg = readData();  // blocking take
        // process + dispatch
    }
}
```

Rust 模块：

```rust
/// 一个模块 = 一个 async fn + 上下游 channel
pub async fn auth(
    mut rx: ChanRx,       // 上游：NATIVE://POLICY_AUTHENTICATION
    classify_tx: ChanTx,  // 下游：NATIVE://POLICY_CLASSIFY
    north_tx: ChanTx,     // 下游：NATIVE://NORTH_ORDER_ASK
    socket_tx: ChanTx,    // 下游：NATIVE://SOCKET_MSG_SENDER
    cfg: AuthConfig,
) -> Result<()> {
    while let Some(msg) = rx.recv().await {
        // 等同于 Java 的 dispose() 循环
        match msg {
            InternalMessage::OrderPolicy(policy) => {
                if authenticate(&policy, &cfg).await {
                    classify_tx.send(InternalMessage::OrderPolicy(policy)).await?;
                } else {
                    tracing::warn!(policy_id = %policy.id(), "鉴权失败");
                    north_tx.send(InternalMessage::PolicyAck(PolicyAck::denied(policy))).await?;
                }
            }
            InternalMessage::SpecialTask(task) => {
                classify_tx.send(InternalMessage::SpecialTask(task)).await?;
            }
            InternalMessage::PolicyAck(ack) => {
                north_tx.send(InternalMessage::PolicyAck(ack)).await?;
            }
            InternalMessage::Text(s) => {
                socket_tx.send(InternalMessage::Text(s)).await?;
            }
            _ => {
                tracing::warn!("auth: 未处理的消息类型");
            }
        }
    }
    Ok(())
}
```

### 5.2 模块合并建议

Java 有 ~30 个 Module，部分可以合并减少任务数：

| Java 模块 | 合并建议 | 理由 |
|---|---|---|
| PolicyReceiveThread + PolicyCMDReceiveThread | **合并** 为 `policy_ingress` | 路由逻辑相同（都走 auth），只是入站队列不同 |
| CheckSinglePolicyThread + CheckPolicyThread + RecollectThread | **合并** 为 `policy_check` | 三者都输出到 `POLICY_TO_TASKGROUP` |
| TaskGroupCreatorThread + TaskUniteThread | **合并** 为 `task_group_assembly` | 线性流水线，无分支 |
| FileDataSendThread + FileReadyNotifyService | **合并** 为 `file_data_manager` | 都处理 FileReadyNotifyVO |
| StatusVOSend + SystemStatusVOSend | **合并** 为 `status_sender` | 都是采集机状态上报 |

合并后约 **15-18 个异步任务**，比 Java 30 个模块少一半。

---

## 六、启动流程（取代 start.sh + gcp.cfg.xml）

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // 1. 加载配置（取代 XML DOM 解析 + ConfigUtil）
    let cfg = Config::from_file("cfg/gcp.toml")?;

    // 2. 初始化日志（取代 log4j.properties）
    tracing_subscriber::fmt()
        .with_env_filter(&cfg.log_filter)
        .init();

    // 3. 连接数据库（取代 HSQLDB JDBC）
    let db = Database::connect(&cfg.database).await?;

    // 4. 构建通道拓扑（取代 DataDispatcher.loadCfg()）
    let pipeline = build_pipeline(&cfg);

    // 5. 启动所有模块（取代线程池 + DataProcessThread）
    run(pipeline, db, cfg).await
}
```

### 配置格式（TOML，取代 XML + properties）

```toml
# cfg/gcp.toml
[server]
host = "0.0.0.0"
port = 9997

[database]
url = "hsqldb:file:data/gcp"
max_connections = 20

[channels]
default_capacity = 50000
socket_sender_capacity = 100000
task_queue_capacity = 20000

[heartbeat]
interval_ms = 5000
timeout_ms = 150000

[tcp]
# 单条 TCP 连接的消息缓冲区（条数，不是字节数）
send_buffer_capacity = 5000
# 启用 Nagle 算法
nagle = false
```

---

## 七、背压策略

Java 版：队列满 → 记录 WARN → 丢弃消息。

Rust 两种选择：

```rust
// 策略 A：丢弃（适用于心跳、状态上报等可丢失消息）
let result = tx.try_send(msg);
if let Err(TrySendError::Full(msg)) = result {
    tracing::warn!("通道 {} 已满，丢弃消息", chan_name);
    // msg 被返回，可记录指标
    METRICS.discarded_total.inc();
}

// 策略 B：阻塞等待（适用于任务指令等必达消息）
if let Err(e) = tx.send(msg).await {
    tracing::error!("通道 {} 已断开: {}", chan_name, e);
    // 触发故障处理
}
```

| 消息类型 | 策略 | 原因 |
|---|---|---|
| `OrderPolicy` / `SpecialTask` | **阻塞** | 丢失意味着任务永远不执行 |
| `TaskStatus` / `Heartbeat` | **丢弃** | 下一条会补上 |
| `FileReadyNotify` | **阻塞** | 丢失意味着数据文件永远不被处理 |
| `SystemStatus` | **丢弃** | 周期性上报，下轮补上 |
| `AdapterRegister` | **阻塞** | 丢失意味着适配器不可用 |

---

## 八、优雅关闭（Java 版缺失）

```rust
pub async fn run(pipeline: Pipeline, mut shutdown: ShutdownSignal) {
    // 每个模块接收 shutdown 信号
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(msg) => process(msg).await,
                    None => break, // 上游关闭
                }
            }
            _ = shutdown.wait() => {
                tracing::info!("收到关闭信号，结束");
                // 1. 停止接收新消息
                // 2. 处理完队列中剩余消息
                // 3. 通知下游关闭
                break;
            }
        }
    }
}
```

关闭顺序：
1. `tcp_listener` 停止接受新连接
2. 各模块按逆序关闭（TCP 出站 → 下游 → 上游）
3. 等待 in-flight 消息处理完成（最多等待 `shutdown_timeout`）
4. 关闭数据库连接

---

## 九、可观测性

### 内置 metrics（取代 CLI `queue -s` + DAL_MQ_STATUS）

```rust
// 每个通道自动暴露
pub struct MonitoredChannel {
    name: &'static str,
    depth: Gauge,        // prometheus gauge: gcp_channel_depth{name="..."}
    sent_total: Counter, // prometheus counter
    discarded_total: Counter,
}

// tokio-console 原生支持 async task 监控
// `RUSTFLAGS="--cfg tokio_unstable" cargo run --features console`
// 即可实时查看每个 channel 深度、task 状态、poll 次数
```

### 关键指标

| 指标 | 来源 | 告警建议 |
|---|---|---|
| `gcp_channel_depth` | 每个 mpsc channel 的 `len()` | 持续 > 80% 容量 → 扩容或消费慢 |
| `gcp_messages_sent_total` | Counter | — |
| `gcp_messages_discarded_total` | Counter | > 0 → 背压不足 |  
| `gcp_tcp_connections_active` | ConnectionRegistry | 与预期采集机数不符 → 连接异常 |
| `gcp_task_duration_seconds` | 每个模块处理时间 | P99 > 1s → 性能瓶颈 |

---

## 十、项目结构

```
gcp-core/
├── Cargo.toml
├── cfg/
│   └── gcp.toml                    # 取代 gcp.cfg.xml + gcp.properties
└── src/
    ├── main.rs                     # 入口：启动 Core 或 Collector
    ├── config.rs                   # TOML 配置解析
    ├── message.rs                  # InternalMessage 枚举 + 子类型定义
    ├── pipeline.rs                 # build_pipeline() + run()
    ├── db/
    │   └── mod.rs                  # 数据库操作（取代 JDBC/HSQLDB）
    ├── tcp/
    │   ├── mod.rs
    │   ├── listener.rs             # 取代 QueueAcceptor
    │   ├── connector.rs            # 取代 QueueClient + QueueFactory
    │   ├── registry.rs             # 取代 DataStore
    │   └── protocol.rs             # 帧格式 + 序列化
    ├── modules/
    │   ├── mod.rs
    │   ├── policy_ingress.rs       # PolicyReceive + CMDReceive
    │   ├── auth.rs                 # AuthenticationThread
    │   ├── classifier.rs           # PolicyClassifyThread
    │   ├── policy_check.rs         # CheckSingle + CheckPolicy + Recollect
    │   ├── task_group_assembly.rs  # TaskGroupCreator + TaskUnite
    │   ├── task_dispatcher.rs      # TaskDispatchThread
    │   ├── tcp_sender.rs           # SocketMsgSndrThread
    │   ├── tcp_receiver.rs         # SocketMsgRcvrThread
    │   ├── status_receiver.rs      # StatusReceiver
    │   ├── heartbeat_monitor.rs    # CheckMDHeartBeatThread
    │   ├── adapter_manager.rs      # AdapterManager
    │   └── file_data_manager.rs    # FileNotify + FileSend
    └── metrics.rs                  # Prometheus / tokio-console 暴露
```

---

## 十一、与 Java 版差异总览

| 维度 | Java 版 | Rust 版 |
|---|---|---|
| **配置** | 5+ XML + properties，GBK 编码 | 1 个 TOML，UTF-8 |
| **消息路由** | datachannel.xml + Groovy + 反射 | enum + match，编译期 |
| **进程内通信** | ArrayBlockingQueue + 自研 NativeChannel | tokio::sync::mpsc |
| **跨进程通信** | QueueAcceptor + DataStore + MessageQueue | TcpStream + LengthDelimitedCodec |
| **序列化** | Java 原生序列化 | bincode（或 protobuf） |
| **条件表达式** | GroovyShell 运行时求值 | Rust pattern guards |
| **模块** | Thread 池 + DataProcessThread.dispose() | async task + select! |
| **容量** | 50000（写死） | 每通道独立配置 |
| **背压** | 满则丢 | 可配置：阻塞/丢弃 |
| **关闭** | kill -9 | 信号驱动优雅关闭 |
| **监控** | DAL_MQ_STATUS 表 + CLI | tokio-console + Prometheus |
| **部署** | zip + JDK 1.8 | 静态链接 binary |
| **文件编码** | GBK | UTF-8（统一） |

---

## 十二、简化重构建议

如果觉得一步到位改动太大，可以分两阶段：

**第一阶段：维持 Java 拓扑，纯 Rust 实现**
- 模块数量、通道拓扑与 Java 完全一致（~30 个 async task）
- 每个 task 对应一个 Java Module，一个 Channel
- TCP 协议兼容 Java 版（能对接 Java Core / Java Collector）
- 目标是"用 Rust 跑通现有的 datachannel.xml 拓扑"

**第二阶段：按 Rust 习惯优化**
- 合并相邻模块（减少任务切换）
- 用 enum match 替代反射路由
- 用 tracing 替代 log4j
- 用 TOML 替代 XML
- 删除 Groovy 依赖
