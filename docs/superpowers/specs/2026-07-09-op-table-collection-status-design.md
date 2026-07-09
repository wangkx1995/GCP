# OP 表采集状态记录设计

## 背景

当前 result.csv 只写一行 TPD 表的采集结果。但 OP 表作为中间表，它的采集状态（成功/失败、行数、时间）没有记录。需要改为多行 result.csv，同时保存 OP 表的采集状态，以便追踪数据流和排查问题。

## 数据流

```
Agent 收到 TPD 主任务 (task_id: task_{sid}_{tbl}_{ts})
  │
  ├─ Phase 1: 解析所有 PM CSV 文件 → 汇总各 OP 表的 row_count
  │             └─ 记录每个 OP 表的 row_count、成功/失败
  │             └─ 所有文件解析完成后，一次性写入 result.csv OP 行
  │
  └─ Phase 2: TPD engine finish → 产出 TPD CSV
                └─ 追加写入 result.csv TPD 行
                └─ 一次性上报所有行到 Core
```

## result.csv 列结构

| 列名 | 说明 |
|---|---|
| `table_name` | 表名（TPD_A 或 OP_EUTRANCELL），天然区分 TPD vs OP |
| `data_time` | 数据时间 |
| `row_count` | 行数（多个 PM CSV 映射到同一 OP 表时，累加值） |
| `success` | 1/0 |
| `collect_time` | 采集完成时间 |
| `task_id` | TPD 行: 原 `task_{sid}_{tbl}_{ts}`; OP 行: `{TPD_task_id}_0`...`{TPD_task_id}_N` |
| `strategy_id` | 从主策略下传 |
| `group_id` | 从主策略下传 |

**设计要点**：
- 多个 PM CSV 文件映射到同一个 OP 表时，result.csv 该 OP 表只有一行，`row_count` 累加
- 每个 OP 表独立写一行，各自标记 `success`
- TPD 行所有字段都填（与 OP 行格式一致）

## Agent 端改动

### 2.1 枚举 OP 表

Agent 加载 TPD rules 时，从 `rules[].groups[].source_table` 提取所有唯一的 OP 表名。

### 2.2 两阶段写入 result.csv

**Phase 1 — OP 行写入**：
- 在 `StreamingTpdEngine` 处理 PM CSV 时，监测 OP 表数据流
- 首个 OP 表数据到达时：创建 result.csv，写 header + OP 行
- 后续 OP 表完成时：以追加模式打开，写 OP 行（跳过 header）
- task_id 生成：`{TPD_task_id}_{oph_index}`

**Phase 2 — TPD 行追加**：
- `StreamingTableWriter::finish()` 时，以追加模式打开 result.csv
- 追加 TPD 行（task_id 保持原值不变）

### 2.3 写入工具函数

`write_result_csv` 改名为 `append_result_row`，支持两种模式：
- `create`: 创建新文件 + 写 header + 写第一行
- `append`: 追加行（不写 header）

### 2.4 Agent 上报

`read_result_rows` 一次性读取 result.csv 所有行（OP + TPD），全部上报给 Core。

## Core 端改动

### 3.1 接收处理

Agent 上报 `TaskResultReport`（多行）→ Core `accept_task_result()` 逐行处理：

| 行类型 | 处理方式 |
|---|---|
| TPD 行（task_id 匹配原格式） | UPDATE 已有的 collect_tasks 行 |
| OP 行（task_id 以 `_数字` 结尾） | INSERT 新的 collect_tasks 行 |

### 3.2 collect_tasks OP 行字段

| 字段 | 值来源 |
|---|---|
| `task_id` | 来自 row（Agent 端生成） |
| `logical_task_key` | 新生成 `strategy_{sid}:{time}:{OP表名}` |
| `strategy_id` | 来自 row |
| `group_id` | 来自 row |
| `config_snapshot_id` | 同主任务 |
| `scan_start_time` | 同主任务 |
| `collector_name` | 同主任务 |
| `assigned_agent_id` | 同主任务 |
| `status` | `SUCCEEDED`（根据 success 字段） |
| `table_name` | 来自 row（OP 表名） |
| `data_time` | 来自 row |
| `row_count` | 来自 row |
| `success` | 来自 row |
| `collect_time` | 来自 row |

### 3.3 关联查询

OP 子任务通过 `group_id` 和 `strategy_id` 关联回主任务。

### 3.4 任务重试

- 每次重试生成新的 TPD `task_id`（含时间戳），OP 子 task_id 也随之不同
- 旧行保留在 `collect_tasks` 中作为历史记录，无需清理
- 查询时按需按 `group_id` + 时间范围过滤

## Grid / 前端影响

### 4.1 Grid 查询

现有 grid 查询 `SELECT ... FROM collect_tasks WHERE strategy_id = ? AND data_time LIKE ?` 不做改动。OP 行有相同 `strategy_id`，自然被查出来。

### 4.2 Grid 渲染

- Grid builder 按 `table_name` 分组生成 row
- TPD 表（`TPD_` 前缀）和 OP 表（`OP_` 前缀）自然成为不同的行，展示在同一 grid 中
- 前端无代码改动

## 错误处理

### 部分 OP 表失败

- 各自独立标记 `success`：成功 = 1，失败 = 0
- TPD 行根据能否完成转换决定 `success`

### OP 完成后 TPD 失败

- OP 行已写入 result.csv，正常上报
- TPD 行 `success=0` 上报
- Core 端：OP 行 INSERT，TPD 行 UPDATE

### 任务重试

- result.csv 完全重建（覆盖写入）
- 旧 OP 行已在上次上报时写入 DB，重试生成新 task_id 写入新行
- 无需清理旧数据

## 涉及文件

| 文件 | 改动 |
|---|---|
| `src/writer.rs` | `write_result_csv` → `append_result_row`，支持创建+追加模式 |
| `src/tpd.rs` / `src/parse_job.rs` | Phase 1 结束点插入 OP 行写入逻辑；TPD finish 后追加 TPD 行 |
| `src/agent/runner.rs` | 上报多行 result |
| `src/core/db.rs` | `accept_task_result` 支持 INSERT OP 行 |
| `src/core_agent_api.rs` | 无改动（result.csv 列已有对应字段） |
| `src/core/grid.rs` | 无改动（自动支持多行） |
| `pm-admin/` | 无改动 |
