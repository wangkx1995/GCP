# Data Collector Unit — Design Spec

## Overview

Add `data_collector_unit` management to the Woyang PM parser Core/Agent system. A unit binds a config snapshot (适配器), agents (采集机), tables, source config, and scheduling into a reusable deployment unit.

## Schema

```sql
CREATE TABLE data_collector_unit (
    id INTEGER PRIMARY KEY,
    unit_name TEXT NOT NULL,
    config_name TEXT NOT NULL,
    config_version TEXT NOT NULL DEFAULT '',
    table_names TEXT NOT NULL DEFAULT '[]',
    agent_ids TEXT NOT NULL DEFAULT '[]',
    data_interval_seconds INTEGER NOT NULL DEFAULT 900,
    collector_interval INTEGER NOT NULL DEFAULT 900,
    task_timeout_seconds INTEGER NOT NULL DEFAULT 3600,
    source_type TEXT NOT NULL DEFAULT 'sftp',
    file_encoding TEXT NOT NULL DEFAULT 'UTF-8',
    remote_pattern TEXT NOT NULL DEFAULT '',
    host TEXT NOT NULL DEFAULT '',
    port INTEGER NOT NULL DEFAULT 22,
    username TEXT NOT NULL DEFAULT '',
    password TEXT NOT NULL DEFAULT '',
    connect_retry INTEGER NOT NULL DEFAULT 3,
    download_retry INTEGER NOT NULL DEFAULT 3,
    download_parallel INTEGER NOT NULL DEFAULT 4,
    retry_interval_secs INTEGER NOT NULL DEFAULT 30,
    connect_timeout_secs INTEGER NOT NULL DEFAULT 30,
    read_timeout_secs INTEGER NOT NULL DEFAULT 300,
    cache_retention_days INTEGER NOT NULL DEFAULT 7,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

- `id`: manually assigned, pre-allocated via `next-id` endpoint (`SELECT MAX(id) + 1`).
- `table_names`/`agent_ids`: JSON arrays stored as text.
- `config_version`: auto-populated on save by looking up the active (`activated_at IS NOT NULL`) snapshot's version for `config_name`. If none found, set to `''`.
- All source config fields are flat columns for direct form binding.

## API Endpoints

### POST /api/data-collector-units/next-id

Pre-allocate next available ID.

**响应 200：**
```json
{ "id": 5 }
```

### GET /api/data-collector-units

List all units.

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
        "remote_pattern": "/data/pm/{YYYY}/{MM}/{DD}/{HH}/{scan_start_time}_*.csv.gz",
        "host": "192.168.1.100",
        "port": 22,
        "username": "collector",
        "password": "******",             // 列表返回时脱敏，仅写操作使用明文
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

### PUT /api/data-collector-units/:id

Create or update a unit (UPSERT by id).

**请求体（排除 `created_at`/`updated_at`，后端自动填充）：**
```json
{
    "unit_name": "机房A-北向指标",
    "config_name": "gnb_pm_v1",
    "table_names": "[\"TPD_A\",\"TPD_B\"]",
    "agent_ids": "[\"agent_abc123\",\"agent_def456\"]",
    "data_interval_seconds": 900,
    "collector_interval": 900,
    "task_timeout_seconds": 3600,
    "source_type": "sftp",
    "file_encoding": "UTF-8",
    "remote_pattern": "/data/pm/{YYYY}/{MM}/{DD}/{HH}/{scan_start_time}_*.csv.gz",
    "host": "192.168.1.100",
    "port": 22,
    "username": "collector",
    "password": "",
    "connect_retry": 3,
    "download_retry": 3,
    "download_parallel": 4,
    "retry_interval_secs": 30,
    "connect_timeout_secs": 30,
    "read_timeout_secs": 300,
    "cache_retention_days": 7
}
```

**校验规则：**
- `config_name` 必须在 `config_snapshots` 表中存在且 `activated_at IS NOT NULL`
- `agent_ids` 中每个 ID 必须在 `agents` 表中存在
- 校验失败返回 400：`{ "error": "config_name 'xxx' not found" }` 或 `{ "error": "agent_ids contains unknown ids: [\"agent_xxx\"]" }`

**响应 200：**
```json
{ "id": 5 }
```

### DELETE /api/data-collector-units/:id

Delete a unit.

**响应 200：**`{ "deleted": true }`
**响应 404：**`{ "error": "unit not found" }`

### GET /api/data-collector-units/config-names?search=xxx

Search active config names (activated snapshots).

**查询参数：**`search` — 可选，对 `name` 字段进行模糊匹配

**响应 200：**
```json
{
    "config_names": [
        { "name": "gnb_pm_v1", "version": "v_20260703_120000" },
        { "name": "gnb_pm_v2", "version": "v_20260704_080000" }
    ]
}
```

### GET /api/data-collector-units/tables?config_name=xxx

List table names for a given config.

**响应 200：**
```json
{
    "tables": ["TPD_A", "TPD_B", "TPD_C"]
}
```

数据来源：`config_tables` 表，关联 `config_snapshots` 取 `config_name` 匹配且 `created_at` 最大（即最新上传）的那批表名。

## TypeScript Types

```typescript
interface DataCollectorUnit {
    id: number;
    unit_name: string;
    config_name: string;
    config_version: string;
    table_names: string;           // JSON array string
    agent_ids: string;             // JSON array string
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

interface NextIdResponse {
    id: number;
}

interface ConfigNameItem {
    name: string;
    version: string;
}

interface ConfigNamesResponse {
    config_names: ConfigNameItem[];
}

interface TablesResponse {
    tables: string[];
}
```

## Frontend

- New sidebar menu item: 采集单元配置
- Page: `AgentConfig` (at `/agent-config`, already wired in router)
- Full-page layout: left/upper area shows unit list table (all columns), right/lower area shows edit form for selected unit
- Create flow: click new → `POST /next-id` → form clears with new id → fill → `PUT /:id`
- Edit flow: click row in list → form populates → modify → `PUT /:id`
- Form fields bind 1:1 with schema columns (excluding `created_at`/`updated_at`)
- Dropdowns: agents from `GET /api/agents` (multi-select), config names from `GET /api/data-collector-units/config-names?search=`, tables from `GET /api/data-collector-units/tables?config_name=` (multi-select, populates after config_name selected)

## Implementation Order

1. Backend: table migration + `CoreDb` CRUD methods
2. Backend: HTTP endpoints in `CoreServer`
3. Frontend: API hooks + form page
4. Update `docs/frontend-api-docs.md`
