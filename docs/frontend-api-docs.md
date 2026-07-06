# Core/Agent 系统前端开发文档

## 概述

本文档描述了 Woyang PM 解析器 Core/Agent 系统的全部 HTTP API 接口，适用于前端开发对接。

### Base URL

| 服务 | Base URL | 说明 |
|------|----------|------|
| Core | `http://<core-host>:18080/api` | 管理端 API |
| Agent | `http://<agent-host>:18081/api` | 代理端 API（通常不直接调用） |

### 通用约定

- 请求/响应均为 `application/json`，除非另有说明
- `POST /api/config-snapshots/upload` 除外：body 为原始 zip 字节
- 时间格式：`"YYYY-MM-DD HH:mm:ss"`（例 `"2026-06-17 15:15:00"`）
- 枚举值使用 SCREAMING_SNAKE_CASE

---

## 目录

1. [配置快照管理](#1-配置快照管理)
2. [代理管理](#2-代理管理)
3. [任务管理](#3-任务管理)
4. [结果查询](#4-结果查询)
5. [Agent 端点](#5-agent-端点)
6. [采集单元配置](#6-采集单元配置)
7. [类型定义](#7-类型定义)
8. [典型业务流程](#8-典型业务流程)

---

## 1. 配置快照管理

### 1.1 上传配置快照

上传 zip 格式的配置文件到 Core。Core 会校验完整性、解压并存储。

```
POST /api/config-snapshots/upload
Content-Type: application/octet-stream
Body: <raw zip bytes>
```

**Zip 必需包含的文件：**

| 文件 | 必需 | 说明 |
|------|------|------|
| `source.toml` | ✅ | 数据源配置（FTP/SFTP） |
| `mapping_dx.ini` | ✅ | 表/列映射 |
| `load.toml` | ✅ | 数据库加载配置 |
| `rules/` | ✅ | 目录，内含 TPD 规则 JSON 文件 |

**响应 200：**
```json
{
  "valid": true,
  "config_snapshot_id": "v_20260703_121445",
  "content_hash": "sha256:e5b5afce94e894674c67d7a7d97b065562db5c7120b24f47b4239b96205892bc",
  "file_count": 5
}
```

**响应 400（校验失败）：**
```json
{
  "valid": false,
  "errors": ["missing required file: source.toml", "missing required directory: rules/"],
  "config_snapshot_id": "v_20260703_121445"
}
```

---

### 1.2 获取配置快照列表

```
GET /api/config-snapshots
```

**响应 200：**
```json
[
  {
    "config_snapshot_id": "v_20260703_121445",
    "content_hash": "sha256:e5b5afce...",
    "version_label": null,
    "is_active": false,
    "file_count": 5,
    "created_at": "2026-07-03 12:14:45",
    "activated_at": null
  },
  {
    "config_snapshot_id": "v_20260703_120000",
    "content_hash": "sha256:abc123...",
    "version_label": null,
    "is_active": true,
    "file_count": 5,
    "created_at": "2026-07-03 12:00:00",
    "activated_at": "2026-07-03 12:05:00"
  }
]
```

按 `created_at` 降序排列。

---

### 1.3 获取单个配置快照

```
GET /api/config-snapshots/:id
```

**路径参数：** `id` = `config_snapshot_id`

**响应 200：** 单个 `ConfigSnapshotMeta` 对象（同上）
**响应 404：** 快照不存在

---

### 1.4 激活配置快照

激活一个快照。Core 会原子切换 `active` 符号链接，并通知所有在线 Agent。

```
POST /api/config-snapshots/:id/activate
```

**响应 200：**
```json
{
  "config_snapshot_id": "v_20260703_121445",
  "active": true,
  "content_hash": "sha256:e5b5afce...",
  "activated_at": "2026-07-03 12:15:00"
}
```

**响应 404：** 快照不存在
**响应 500：** 磁盘或数据库错误

---

### 1.5 下载配置快照

下载快照的 zip 归档。

```
GET /api/config-snapshots/:id/download
```

**响应 200：** `Content-Type: application/zip`，附件形式下载
**响应 404：** 快照在磁盘上不存在

---

## 2. 代理管理

### 2.1 注册 Agent

```
POST /api/agents/register
Content-Type: application/json
```

**请求体：**
```json
{
  "agent_id": null,
  "agent_name": "agent-1",
  "host": "192.168.1.100",
  "port": 18081,
  "version": "1.0.0",
  "capabilities": {
    "can_collect": true,
    "can_parse": true,
    "can_load": false,
    "supported_protocols": ["ftp", "sftp"]
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `agent_id` | `string` 或 `null` | `null` 时由服务端自动分配 |
| `agent_name` | `string` | |
| `host` | `string` | Agent 的 IP 或主机名 |
| `port` | `number` | Agent HTTP 端口 |
| `version` | `string` | 软件版本 |
| `capabilities` | `object` | 见下 |

**capabilities 字段：**
| 字段 | 类型 | 说明 |
|------|------|------|
| `can_collect` | `boolean` | |
| `can_parse` | `boolean` | |
| `can_load` | `boolean` | |
| `supported_protocols` | `string[]` | 例如 `["ftp", "sftp"]` |

**响应 200：**
```json
{
  "agent_id": "agent_abc123",
  "heartbeat_interval_seconds": 10,
  "task_report_interval_seconds": 10
}
```

---

### 2.2 Agent 心跳

```
POST /api/agents/:agent_id/heartbeat
```

**当前为桩，直接返回：**
```json
{ "accepted": true }
```

---

## 3. 任务管理

### 3.1 分发任务

Core 先选择一个在线 Agent，将任务持久化到 DB，然后 HTTP 转发到 Agent。

```
POST /api/tasks/dispatch
Content-Type: application/json
```

**请求体：**
```json
{
  "task_id": "task_20260703_001",
  "logical_task_key": "strategy_1:2026-06-17 15:15:00:v_20260703_120000",
  "strategy_id": "strategy_1",
  "config_snapshot_id": "v_20260703_120000",
  "scan_start_time": "2026-06-17 15:15:00",
  "collect_id": "collect_001",
  "load_type": "clickhouse",
  "encoding": "UTF-8",
  "output_delimiter": "|",
  "timeout_seconds": 1800,
  "callback_base_url": "http://127.0.0.1:18080/api"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `task_id` | `string` | 全局唯一任务 ID |
| `logical_task_key` | `string` | 用于去重 |
| `strategy_id` | `string` | 策略 ID |
| `config_snapshot_id` | `string` | 使用的配置版本 |
| `scan_start_time` | `string` | 数据时间 |
| `collect_id` | `string` | 采集运行标识 |
| `load_type` | `string` | `"clickhouse"` 或 `"postgresql"` |
| `encoding` | `string` | 例如 `"UTF-8"` |
| `output_delimiter` | `string` | 输出文件分隔符 |
| `timeout_seconds` | `number` | 超时时间 |
| `callback_base_url` | `string` | 结果回传的 Core URL |

**响应 200：**
```json
{
  "task_id": "task_20260703_001",
  "accepted": true,
  "agent_task_state": "ACCEPTED",
  "reason": null
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `accepted` | `boolean` | Agent 是否接受 |
| `agent_task_state` | `string` | `ACCEPTED` 或 `FAILED` |
| `reason` | `string` 或 `null` | 拒绝原因 |

**响应 503：** 没有在线 Agent
**响应 502：** Agent 不可达
**响应 500：** 数据库错误

---

### 3.2 上报结果

Agent 执行完成后通过此接口回传结果。

```
POST /api/tasks/:task_id/result
Content-Type: application/json
```

**请求体：**
```json
{
  "task_id": "task_20260703_001",
  "agent_id": "agent_abc123",
  "status": "SUCCEEDED",
  "result_rows": [
    {
      "table_name": "TPD_A",
      "data_time": "2026-06-17 15:15:00",
      "row_count": 12345,
      "success": 1,
      "collect_time": "2026-07-02 15:35:00"
    }
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `status` | `string` | `SUCCEEDED` / `FAILED` / `TIMEOUT` / `CANCELLED` |
| `result_rows[].table_name` | `string` | 表名 |
| `result_rows[].data_time` | `string` | 数据时间 |
| `result_rows[].row_count` | `number` | 行数 |
| `result_rows[].success` | `number` | `0` 或 `1` |
| `result_rows[].collect_time` | `string` | 采集完成时间 |

**响应 200：**
```json
{ "accepted": true }
```

---

### 3.3 任务事件（桩）

```
POST /api/tasks/:task_id/events
```

**当前为桩，直接返回：**
```json
{ "accepted": true }
```

预留的事件上报接口，预期请求体格式：
```json
{
  "agent_id": "agent_abc123",
  "event_id": "evt_001",
  "status": "RUNNING",
  "phase": "downloading",
  "message": "download started"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `status` | `string` | `TaskStatus`（SCREAMING_SNAKE_CASE） |
| `phase` | `string` 或 `null` | `preparing_config` / `downloading` / `parsing` / `writing_output` / `reporting_result` |
| `message` | `string` 或 `null` | 详细消息 |

---

## 4. 结果查询

### 4.1 每日结果网格

按策略和日期查询采集结果概览，以时间槽网格形式返回。

```
GET /api/results/grid?strategy_id=strategy_1&day=2026-06-17&interval_minutes=15
```

**查询参数：**

| 参数 | 类型 | 必需 | 默认 | 说明 |
|------|------|------|------|------|
| `strategy_id` | `string` | 是 | — | |
| `day` | `string` | 是 | — | 日期 `"YYYY-MM-DD"` |
| `interval_minutes` | `number` | 否 | `15` | 时间槽间隔（分钟） |

**响应 200：**
```json
{
  "day": "2026-06-17",
  "time_slots": [
    "2026-06-17 00:00:00",
    "2026-06-17 00:15:00",
    "..."
  ],
  "rows": [
    {
      "table_name": "TPD_A",
      "cells": [
        {
          "data_time": "2026-06-17 00:00:00",
          "value": 12345,
          "color": "green",
          "status": "ok"
        },
        {
          "data_time": "2026-06-17 00:15:00",
          "value": null,
          "color": "gray",
          "status": "missing"
        }
      ]
    }
  ]
}
```

**颜色规则：**

| color | status | 含义 |
|-------|--------|------|
| `green` | `ok` | 数据正常（row_count > 0） |
| `yellow` | `empty` | 采集成功但 0 行数据 |
| `red` | `failed` | 采集失败（success = 0） |
| `gray` | `missing` | 没有数据（未采集或时间槽未到） |

| time_slots 数量 | interval_minutes | slots |
|-----------------|------------------|-------|
| | 15 | 96 |
| | 30 | 48 |
| | 60 | 24 |

---

## 5. Agent 端点

下面两个端点由 Core 调用，前端通常不直接调用。

### 5.1 Agent 接收任务

```
POST /api/tasks
Content-Type: application/json
```

请求体同 Core 的 `TaskDispatchRequest`。

响应体同 Core 的 `TaskDispatchResponse`。

Agent 的行为：
1. 如需下载配置（`--config-dir` 未设置时）
2. 从 Core 的 `/api/config-snapshots/{id}/download` 拉取 zip
3. 解压到 `data_dir/config_snapshots/{snapshot_id}/`
4. 将配置复制到任务目录
5. 后台异步启动 parse_job
6. 执行完成后回传结果到 Core 的 `/api/tasks/{task_id}/result`

---

### 5.2 Agent 热更新通知

Core 激活配置时调用，通知 Agent 有新配置可用。

```
POST /api/configs/update
Content-Type: application/json
```

**请求体：**
```json
{
  "snapshot_id": "v_20260703_121445",
  "content_hash": "sha256:e5b5afce..."
}
```

**响应 200：**
```json
{ "accepted": true }
```

Agent 后台异步从 Core 下载新配置到本地缓存。不影响正在运行的任务。

---

## 6. 采集单元配置

### 6.1 预分配 ID

```
POST /api/data-collector-units/next-id
```

**响应 200：**
```json
{ "id": 5 }
```

### 6.2 获取采集单元列表

```
GET /api/data-collector-units
```

**响应 200：**
```json
[
  {
    "id": 5,
    "unit_name": "机房A-北向指标",
    "config_name": "gnb_pm_v1",
    "config_version": "v_20260703_120000",
    "table_names": "[\"TPD_A\",\"TPD_B\"]",
    "agent_ids": "[\"agent_abc123\",\"agent_def456\"]",
    "data_interval_seconds": 900,
    "collector_interval": 900,
    "task_timeout_seconds": 3600,
    "source_type": "sftp",
    "file_encoding": "UTF-8",
    "remote_pattern": "/data/pm/{scan_start_time}_*.csv.gz",
    "host": "192.168.1.100",
    "port": 22,
    "username": "collector",
    "password": "******",
    "connect_retry": 3,
    "download_retry": 3,
    "download_parallel": 4,
    "retry_interval_secs": 30,
    "connect_timeout_secs": 30,
    "read_timeout_secs": 300,
    "cache_retention_days": 7,
    "created_at": "2026-07-04 10:00:00",
    "updated_at": "2026-07-04 10:00:00"
  }
]
```

### 6.3 保存采集单元（新建/更新）

```
PUT /api/data-collector-units/:id
Content-Type: application/json
```

**请求体（排除 `created_at`/`updated_at`，后端自动填充）：**

同列表响应体，去掉 `id`（在 URL 中）、`config_version`（自动填充）、`created_at`、`updated_at`。

**校验规则：**
- `config_name` 必须已激活
- `agent_ids` 中所有 ID 必须存在
- 密码留空或 `"******"` 保留原值

**响应 200：**
```json
{ "id": 5 }
```

**响应 400：**
```json
{ "error": "config_name 'xxx' not found or not active" }
```

### 6.4 删除采集单元

```
DELETE /api/data-collector-units/:id
```

**响应 200：**
```json
{ "deleted": true }
```

**响应 404：**
```json
{ "error": "采集单元 123 不存在" }
```

### 6.5 搜索适配器名称

```
GET /api/data-collector-units/config-names?search=xxx
```

**查询参数：**`search` 可选，模糊匹配

**响应 200：**
```json
{
  "config_names": [
    { "name": "gnb_pm_v1", "version": "v_20260703_120000" }
  ]
}
```

### 6.6 获取适配器表名

```
GET /api/data-collector-units/tables?config_name=gnb_pm_v1
```

**响应 200：**
```json
{
  "tables": ["TPD_A", "TPD_B", "TPD_C"]
}
```

---

## 7. 类型定义

### 枚举

**TaskStatus：**
```
CREATED | DISPATCHING | ACCEPTED | RUNNING | SUCCEEDED | FAILED | TIMEOUT | CANCEL_REQUESTED | CANCELLED
```

**AgentStatus：**
```
ONLINE | UNKNOWN | OFFLINE
```

**TaskPhase：**
```
preparing_config | downloading | parsing | writing_output | reporting_result
```

### 数据模型

```typescript
// 配置快照
interface ConfigSnapshotMeta {
  config_snapshot_id: string;      // "v_20260703_121445"
  content_hash: string;            // "sha256:..."
  version_label: string | null;
  is_active: boolean;
  file_count: number;
  created_at: string;              // "2026-07-03 12:14:45"
  activated_at: string | null;
}

// 上传响应（成功）
interface UploadSuccessResponse {
  valid: true;
  config_snapshot_id: string;
  content_hash: string;
  file_count: number;
}

// 上传响应（失败）
interface UploadErrorResponse {
  valid: false;
  errors: string[];
  config_snapshot_id: string;
}

// 激活响应
interface ActivateResponse {
  config_snapshot_id: string;
  active: true;
  content_hash: string;
  activated_at: string | null;
}

// Agent 注册
interface AgentRegisterRequest {
  agent_id: string | null;
  agent_name: string;
  host: string;
  port: number;
  version: string;
  capabilities: AgentCapabilities;
}

interface AgentCapabilities {
  can_collect: boolean;
  can_parse: boolean;
  can_load: boolean;
  supported_protocols: string[];
}

interface AgentRegisterResponse {
  agent_id: string;
  heartbeat_interval_seconds: number;
  task_report_interval_seconds: number;
}

// 任务分发
interface TaskDispatchRequest {
  task_id: string;
  logical_task_key: string;
  strategy_id: string;
  config_snapshot_id: string;
  scan_start_time: string;
  collect_id: string;
  load_type: string;
  encoding: string;
  output_delimiter: string;
  timeout_seconds: number;
  callback_base_url: string;
}

interface TaskDispatchResponse {
  task_id: string;
  accepted: boolean;
  agent_task_state: string;  // TaskStatus
  reason: string | null;
}

// 结果回传
interface TaskResultReport {
  task_id: string;
  agent_id: string;
  status: string;             // TaskStatus
  result_rows: ResultRow[];
}

interface ResultRow {
  table_name: string;
  data_time: string;
  row_count: number;
  success: number;           // 0 or 1
  collect_time: string;
}

// 结果网格
interface DailyGrid {
  day: string;
  time_slots: string[];
  rows: TableGridRow[];
}

interface TableGridRow {
  table_name: string;
  cells: GridCell[];
}

interface GridCell {
  data_time: string;
  value: number | null;
  color: "green" | "yellow" | "red" | "gray";
  status: "ok" | "empty" | "failed" | "missing";
}

// 热更新通知
interface ConfigUpdateRequest {
  snapshot_id: string;
  content_hash: string;
}

// 任务事件（预留）
interface TaskEventRequest {
  agent_id: string;
  event_id: string;
  status: string;              // TaskStatus
  phase: string | null;        // TaskPhase
  message: string | null;
}

// 采集单元
interface DataCollectorUnit {
  id: number;
  unit_name: string;
  config_name: string;
  config_version: string;
  table_names: string;
  agent_ids: string;
  data_interval_seconds: number;
  collector_interval: number;
  task_timeout_seconds: number;
  source_type: string;
  file_encoding: string;
  remote_pattern: string;
  host: string;
  port: number;
  username: string;
  password: string;
  connect_retry: number;
  download_retry: number;
  download_parallel: number;
  retry_interval_secs: number;
  connect_timeout_secs: number;
  read_timeout_secs: number;
  cache_retention_days: number;
  created_at: string;
  updated_at: string;
}

interface DataCollectorUnitSaveRequest {
  unit_name: string;
  config_name: string;
  table_names: string;
  agent_ids: string;
  data_interval_seconds?: number;
  collector_interval?: number;
  task_timeout_seconds?: number;
  source_type?: string;
  file_encoding?: string;
  remote_pattern?: string;
  host?: string;
  port?: number;
  username?: string;
  password?: string;
  connect_retry?: number;
  download_retry?: number;
  download_parallel?: number;
  retry_interval_secs?: number;
  connect_timeout_secs?: number;
  read_timeout_secs?: number;
  cache_retention_days?: number;
}
```

---

## 8. 典型业务流程

### 业务流程：上传配置 → 激活 → 查看结果

```
1. 上传 zip ──────────────────► POST /api/config-snapshots/upload
                                  │
2. 获取快照 ID ◄────────────────┘ {"config_snapshot_id": "v_..."}
                                  │
3. 激活快照 ──────────────────► POST /api/config-snapshots/:id/activate
                                  │
4. Agent 自动下载配置 ◄────────┘ Core 通知所有在线 Agent
                                  │
5. 分发采集任务 ──────────────► POST /api/tasks/dispatch
                                  │
6. Agent 执行并回传结果 ◄──────┘ POST /api/tasks/:task_id/result
                                  │
7. 查看当日网格 ──────────────► GET /api/results/grid?strategy_id=...&day=...
                                  │
8. 展示绿色/红色网格 ◄────────┘
```

### 场景：管理员管理配置

前端应该实现的功能：

1. **配置列表页** — `GET /api/config-snapshots`
   - 表格展示所有快照（ID、hash、激活状态、创建时间）
   - 高亮当前激活的快照（`is_active: true`）
   - "上传"按钮触发文件选择

2. **配置上传** — `POST /api/config-snapshots/upload`
   - 选择本地的 .zip 文件
   - 用 `Content-Type: application/octet-stream` 上传
   - 成功后自动刷新列表
   - 失败时展示校验错误（`errors` 数组）

3. **配置激活/回滚** — `POST /api/config-snapshots/:id/activate`
   - 点击非激活的快照的"激活"按钮
   - 确认弹窗后调用
   - 成功后列表高亮切换

4. **配置下载** — `GET /api/config-snapshots/:id/download`
   - 以 zip 附件形式下载

5. **结果网格页** — `GET /api/results/grid`
   - 选择策略 + 日期
   - 渲染颜色网格（红色/绿色/灰色）
   - 鼠标悬停显示具体行数

### 场景：Agent 管理

6. **Agent 注册** — `POST /api/agents/register`
   - 管理界面手工注册 Agent
   - 或 Agent 启动时自动注册（前端不涉及）

7. **任务分发** — `POST /api/tasks/dispatch`
   - 前端组装 TaskDispatchRequest
   - 展示分发结果（accepted/rejected）

---

## 附录：错误处理

| HTTP 状态码 | 含义 | 常见场景 |
|-------------|------|----------|
| 200 | 成功 | |
| 400 | 请求错误 | zip 格式错误、缺少必需文件 |
| 404 | 未找到 | 快照 ID 不存在 |
| 500 | 服务器内部错误 | 数据库异常、磁盘 I/O 错误 |
| 502 | 网关错误 | Core 转发任务到 Agent 失败 |
| 503 | 服务不可用 | 没有在线 Agent |

Agent 端不会返回 HTTP 错误码 —— 所有失败通过 `TaskDispatchResponse.accepted` 和 `reason` 传递。

---

## 附录：编译类型对照

| Rust 类型 | JSON 类型 | 说明 |
|-----------|-----------|------|
| `String` | `string` | |
| `u64`, `u16`, `usize` | `number` | 不超过 JS Number 安全范围 |
| `i32` | `number` | |
| `bool` | `boolean` | |
| `Option<T>` | `T` 或 `null` | |
| `Vec<T>` | `T[]` | |
| `HashMap / BTreeSet` | `object` / `array` | |
| `bytes::Bytes` | 二进制 | 仅 download 端点和 upload 请求体 |
