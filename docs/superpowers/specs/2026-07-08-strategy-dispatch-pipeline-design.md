# Strategy Dispatch Pipeline Design

## Goal

完善采集策略下发流程，将实时策略、周期策略、补采策略统一进入同一套内部调度链路。第一阶段实现重点是统一入口、校验、任务组构建、负载均衡、延迟重派、TCP 下发和心跳失败处理。

任务组不新建独立表，使用 `collect_tasks.group_id` 做自关联。同一任务组内的多条 `collect_tasks` 记录写相同 `group_id`。

## Current State

现有代码已经具备这些基础能力：

- `collection_strategy` 保存采集策略。
- `data_collector_unit` 保存采集单元和候选采集机或机组。
- `agent_info`、`agent_status`、`agent_group` 保存采集机、心跳、机组。
- `collect_tasks` 保存单个采集任务。
- `ConnectionRegistry` 为在线 Agent 维护 TCP 长连接。
- `InternalMessage::DispatchTask(TaskDispatchRequest)` 支持 Core 向 Agent 下发单任务。
- Agent 收到 `DispatchTask` 后会持久化、下载配置、执行采集、回传结果。

当前主要不足：

- 实时策略和 `/api/tasks/dispatch` 直接选机发送，没有统一入站通道。
- 周期策略没有每分钟扫描和到期生成指令。
- 补采机制没有接入标准下发路径。
- 选机逻辑只是选择最近在线 Agent，没有容量、负载、机组展开、强制路由、重试。
- `TaskEvent` 和 `DispatchTaskAck` 没有完整更新任务状态。
- 心跳超时只标记 Agent 离线，没有失败该 Agent 上的运行中或待分派任务。

## Scope

本设计第一阶段实现 Core 内部任务组调度，但 TCP 协议暂时仍拆成多个现有 `DispatchTask` 下发。Agent 可以不感知任务组。

不在第一阶段实现：

- 新建任务组表。
- 新增 `DispatchTaskGroup` TCP 消息。
- Agent 一次接收并执行多表任务组。
- 完整 UI 改版。

这些能力可以在后续阶段基于 `collect_tasks.group_id` 平滑扩展。

## Data Model

`collect_tasks` 增加任务组字段：

```sql
ALTER TABLE collect_tasks ADD COLUMN group_id TEXT;
```

为支持可靠重试，建议同时增加：

```sql
ALTER TABLE collect_tasks ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE collect_tasks ADD COLUMN next_retry_at TEXT;
ALTER TABLE collect_tasks ADD COLUMN dispatch_error TEXT;
```

字段语义：

- `task_id`：单个实际执行任务 ID。
- `group_id`：任务组 ID，同一组内多个任务共享相同值。
- `assigned_agent_id`：最终选中的 Agent ID。
- `retry_count`：该任务所属组的重派次数。组内任务保持一致。
- `next_retry_at`：延迟重派时间。组内任务保持一致。
- `dispatch_error`：最近一次分派失败原因。

`group_id` 使用独立生成值，不使用第一条 `task_id`。推荐生成方式：

```text
group_{crc64(strategy_id + collector_id + scan_start_time + scan_end_time + sorted_table_names)}
```

同一策略、同一采集单元、同一时间窗口、同一批表名会生成稳定组 ID，便于幂等、防重复和补采重试。

## Pipeline

### 1. Strategy Ingress

新增 Core 内部 `StrategyCommand`，三类来源统一发送到同一个 `mpsc` 通道：

- 实时采集策略接口创建策略后构造 `StrategyCommand`。
- 周期扫描任务每分钟扫描 `collection_strategy`，到期后构造 `StrategyCommand`。
- 补采线程发现缺失窗口后构造 `StrategyCommand`。

`StrategyCommand` 至少包含：

- 来源：`immediate`、`periodic`、`backfill`。
- 策略 ID 或策略行。
- 采集单元 ID 或名称。
- 时间窗口。
- 表名列表。
- 可选强制 Agent ID。

### 2. Validation

入站命令统一校验：

- 采集类型分类，第一阶段仅支持文件类，消息类保留枚举和错误提示。
- 采集单元存在。
- 激活配置快照存在。
- 时间窗口合法。
- 候选 Agent ID 或 Group ID 可解析。
- 候选 Agent 未禁用。

校验失败时记录告警日志，并将已生成的任务更新为 `FAILED`。若尚未落任务，只返回接口错误或记录调度错误。

### 3. Task Group Build And Merge

Core 内部构造 `TaskGroup` 内存对象：

- `group_id`
- `source_strategy_ids`
- `collector_unit_id`
- `collector_unit_name`
- `candidate_ids`
- `scan_start_time`
- `scan_end_time`
- `table_names`
- `config_snapshot_id`
- `force_agent_id`
- `retry_count`

落库时每个表仍创建一条 `collect_tasks`，但写入同一个 `group_id`。

合并规则：同一候选范围、同一时间窗口、同一采集单元和同一配置快照的多个任务组合并。合并后重新按合并内容计算 `group_id`，并把组内任务行更新为新的 `group_id`。

第一阶段合并后仍拆成多个 `DispatchTask` 下发，减少 Core 侧选机和重试开销。Agent 侧调度开销后续通过 `DispatchTaskGroup` 再优化。

### 4. Load Balancing

分发线程对合并后的 `TaskGroup` 选机。流程：

1. 如果策略参数携带强制 Agent ID，跳过评分算法，直接路由到该 Agent，但仍需做在线和容量校验。
2. 如果候选列表只有一个元素且它是 Group ID，查询 `agent_group.agent_ids` 展开为组内 Agent。
3. 遍历候选 Agent，执行三类检查：
   - TCP registry 存在连接，且 DB 心跳时间在 150 秒内。
   - `cpu_load` 和 `memory_load` 低于 `host_load_limit`。
   - 当前非终态任务数加本组新任务数不超过 `agent_power`。
4. 对通过检查的 Agent 评分，选最高分。

评分公式采用：

```text
score = (agent_power / max(total_task_count, 1)) * factor - running_task_count
```

其中：

- `running_task_count` 是该 Agent 当前非终态任务数。
- `total_task_count = running_task_count + new_task_count`。
- `factor` 第一阶段使用常量 `1.0`，后续可配置。

如果没有候选 Agent 通过检查，将整个 `group_id` 加入延迟重派队列。

### 5. Delayed Redispatch

重派按 `group_id` 处理。

每次失败：

- `retry_count += 1`
- 写入 `dispatch_error`
- 写入 `next_retry_at`
- 记录告警日志

最多重试 10 次。超过 10 次后，将该 `group_id` 下非终态任务更新为 `FAILED`。

第一阶段可以使用内存延迟队列驱动重试，同时将重试字段落库。Core 重启后可以扫描 `next_retry_at <= now` 且非终态的任务组恢复重派。

### 6. TCP Send

分发线程不直接调用 `registry.send()`，统一向已有 `to_tcp` 通道写入 `(agent_id, InternalMessage::DispatchTask(...))`。

`tcp_sender_loop` 负责阻塞读取并通过 `ConnectionRegistry` 按 `agent_id` 发送。现有系统使用 CRC64 `agent_id` 作为稳定寻址键，继续保留，不改成 Agent 名称寻址。

### 7. Agent Receive And Execute

第一阶段 Agent 仍接收多个单任务 `DispatchTask`。

可选给 `TaskDispatchRequest` 增加：

```rust
pub group_id: Option<String>
```

Agent 不需要理解任务组语义，只用于日志、本地任务文件和问题定位。

Agent 后续应补充：

- 收到任务后发送 `DispatchTaskAck`。
- 开始执行后发送 `TaskEvent(RUNNING)`。
- 心跳上报真实 `running_task_ids`。
- 使用 `max_concurrent_tasks` 控制并发。

### 8. Result And Heartbeat Recovery

Core 处理 Agent 消息：

- `DispatchTaskAck` 更新任务为 `ACCEPTED`。
- `TaskEvent(RUNNING)` 更新任务为 `RUNNING`。
- `TaskResult` 继续调用 `accept_task_result()` 写结果和终态。

心跳清理线程每 5 秒或按配置扫描 TCP registry。若 Agent 超过 150 秒未心跳：

- unregister TCP 连接。
- 标记 Agent `OFFLINE`。
- 将该 Agent 上 `DISPATCHING`、`ACCEPTED`、`RUNNING` 状态任务更新为 `FAILED`。
- 写入 `dispatch_error = 'agent heartbeat timeout'`。

补采机制后续根据 `collect_result_cells` 和策略窗口发现缺失后，重新构造 `StrategyCommand` 进入同一入站通道。

## Required Code Changes

### `src/core/db.rs`

新增或修改：

- `collect_tasks` schema 增加 `group_id`、`retry_count`、`next_retry_at`、`dispatch_error`。
- `create_task()` 增加 `group_id` 参数。
- `assign_group_to_agent(group_id, agent_id)`。
- `update_group_status(group_id, status, error_message)`。
- `increment_group_retry(group_id, next_retry_at, error_message)`。
- `count_active_tasks_by_agent(agent_id)`。
- `mark_active_tasks_failed_for_agent(agent_id, reason)`。
- `list_candidate_agents(candidate_ids)`。
- `expand_agent_group(group_id)`。

所有静态 SQL 继续使用 `trace_sql!`。

### `src/core/server.rs`

新增或修改：

- `StrategyCommand`、`TaskGroup` 内部类型。
- 策略入站通道和处理 loop。
- 周期策略扫描 loop。
- 任务组构建、合并、分发 loop。
- 延迟重派 loop。
- 负载均衡函数或模块。
- `create_strategies()` 中 immediate 分支改为发送 `StrategyCommand`。
- `dispatch_task()` 改为复用统一调度路径，或保留为底层测试接口但不作为主路径。
- `tcp_cleanup_loop()` 增加任务失败处理。
- `tcp_dispatch_loop()` 处理 `DispatchTaskAck` 和 `TaskEvent` 状态更新。

### `src/core_agent_api.rs`

可选新增：

- `TaskDispatchRequest.group_id: Option<String>`。

### `src/message.rs`

第一阶段不新增消息类型。后续任务组 TCP 下发再新增 `DispatchTaskGroup`。

### `src/agent/server.rs` 和 `src/agent/tcp.rs`

第一阶段最小改动：

- 可选记录 `group_id`。
- 发送 `DispatchTaskAck`。
- 心跳上报真实运行任务列表。

## Tests

Rust 单元测试重点：

- `group_id` 生成稳定且表名排序不影响结果。
- Group ID 展开 Agent Group 正确。
- 负载过滤排除离线、超心跳、超负载、超容量 Agent。
- 评分选择最空闲 Agent。
- 无可用 Agent 时增加重试次数并设置 `next_retry_at`。
- 超过 10 次重试后整组任务失败。
- 心跳超时会失败该 Agent 上非终态任务。
- `TaskEvent` 和 `DispatchTaskAck` 正确更新任务状态。

集成验证：

```bash
cargo test
```

若只改 Core 调度，可先跑聚焦测试，再跑全量测试。

## Open Decisions

第一阶段已确定：

- 不新建任务组表。
- 使用 `collect_tasks.group_id` 自关联。
- `group_id` 使用独立生成值。
- Core 内部任务组调度，TCP 暂时拆成多个 `DispatchTask`。

后续待定：

- 是否新增 `DispatchTaskGroup` 让 Agent 一次接收任务组。
- UI 是否需要按 `group_id` 展示任务组视图。
- 负载均衡 `factor` 是否开放为配置项。
