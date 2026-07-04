# 采集策略管理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add collection strategy management with immediate/periodic types, CRUD, batch suspend/activate.

**Architecture:** Backend: `collection_strategy` table in SQLite via sqlx, HTTP handlers in CoreServer. Frontend: list page with checkbox batch ops + two create forms reusing existing patterns.

**Tech Stack:** Rust + sqlx + axum + React + Ant Design + TanStack Query

## Global Constraints

- `serialize_json_string` function already exists in `core_agent_api.rs` for JSON string → native array serialization
- All columns center-aligned in list tables (existing CSS convention)
- `agent_ids` stored as TEXT JSON string, serialized as native array in API response via `#[serde(serialize_with = "serialize_json_string")]`
- One strategy row per table_name; create with N table_names generates N rows
- Strategy type: `immediate` or `periodic` (string values)
- Status: `可用` or `挂起` (string values)

---
### Task 1: Backend — Shared Types

**Files:**
- Modify: `src/core_agent_api.rs`

**Interfaces:**
- Produces: `CollectionStrategyRow`, `CollectionStrategyCreateRequest`, `CollectionStrategyUpdateRequest`, `BatchStatusRequest` structs

- [ ] **Step 1: Add strategy types after existing `DataCollectorUnitSaveRequest`**

```rust
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CollectionStrategyRow {
    pub id: i64,
    pub collector_name: String,
    pub collector_id: i64,
    pub table_name: String,
    pub status: String,
    pub cron_expression: String,
    pub collect_interval: i64,
    pub data_interval: i64,
    pub data_start_time: Option<String>,
    pub data_end_time: Option<String>,
    pub execute_time: Option<String>,
    #[serde(serialize_with = "serialize_json_string")]
    pub agent_ids: String,
    pub strategy_type: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CollectionStrategyCreateRequest {
    pub collector_id: i64,
    pub collector_name: String,
    pub table_names: Vec<String>,
    pub cron_expression: Option<String>,
    pub collect_interval: i64,
    pub data_interval: i64,
    pub data_start_time: Option<String>,
    pub data_end_time: Option<String>,
    pub execute_time: Option<String>,
    pub agent_ids: String,
    pub strategy_type: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CollectionStrategyUpdateRequest {
    pub cron_expression: Option<String>,
    pub collect_interval: Option<i64>,
    pub data_interval: Option<i64>,
    pub data_start_time: Option<String>,
    pub data_end_time: Option<String>,
    pub execute_time: Option<String>,
    pub agent_ids: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BatchStatusRequest {
    pub ids: Vec<i64>,
}
```

- [ ] **Step 2: Verify build**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/core_agent_api.rs
git commit -m "feat: add collection strategy shared types"
```

### Task 2: Backend — DB Schema + CRUD

**Files:**
- Modify: `src/core/db.rs`

**Interfaces:**
- Consumes: `CollectionStrategyRow`, `CollectionStrategyCreateRequest`, `CollectionStrategyUpdateRequest`, `BatchStatusRequest`
- Produces: `next_strategy_id() -> i64`, `create_strategies() -> Vec<CollectionStrategyRow>`, `list_strategies()`, `get_strategy()`, `update_strategy()`, `batch_suspend()`, `batch_activate()`

- [ ] **Step 1: Add `collection_strategy` table creation in `ensure_schema` after `data_collector_unit` block (around line 160)**

```rust
// collection_strategy
"CREATE TABLE IF NOT EXISTS collection_strategy (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    collector_name TEXT NOT NULL,
    collector_id INTEGER NOT NULL,
    table_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT '可用',
    cron_expression TEXT NOT NULL DEFAULT '',
    collect_interval INTEGER NOT NULL,
    data_interval INTEGER NOT NULL,
    data_start_time TEXT,
    data_end_time TEXT,
    execute_time TEXT,
    agent_ids TEXT NOT NULL DEFAULT '[]',
    strategy_type TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
)",
```

- [ ] **Step 2: Add `next_strategy_id` method after existing `next_unit_id`**

```rust
pub async fn next_strategy_id(&self) -> Result<i64> {
    let id: Option<i64> = sqlx::query_scalar("SELECT COALESCE(MAX(id),0)+1 FROM collection_strategy")
        .fetch_one(&self.pool)
        .await?;
    Ok(id.unwrap_or(1))
}
```

- [ ] **Step 3: Add `create_strategies` method**

```rust
pub async fn create_strategies(&self, req: &CollectionStrategyCreateRequest) -> Result<Vec<CollectionStrategyRow>> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    for table_name in &req.table_names {
        sqlx::query(
            "INSERT INTO collection_strategy (collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at) VALUES (?, ?, ?, '可用', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&req.collector_name)
        .bind(req.collector_id)
        .bind(table_name)
        .bind(req.cron_expression.as_deref().unwrap_or(""))
        .bind(req.collect_interval)
        .bind(req.data_interval)
        .bind(&req.data_start_time)
        .bind(&req.data_end_time)
        .bind(&req.execute_time)
        .bind(&req.agent_ids)
        .bind(&req.strategy_type)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
    }
    // Return created rows
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM collection_strategy ORDER BY id DESC LIMIT ?")
        .bind(req.table_names.len() as i64)
        .fetch_all(&self.pool)
        .await?;
    let mut rows = Vec::new();
    for id in ids.iter().rev() {
        rows.push(self.get_strategy(*id).await?.unwrap());
    }
    Ok(rows)
}

pub async fn get_strategy(&self, id: i64) -> Result<Option<CollectionStrategyRow>> {
    let row = sqlx::query_as::<_, CollectionStrategyRow>(
        "SELECT id, collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at FROM collection_strategy WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&self.pool)
    .await?;
    Ok(row)
}
```

- [ ] **Step 4: Add `list_strategies` method**

```rust
pub async fn list_strategies(&self, collector_name: Option<&str>, strategy_type: Option<&str>, status: Option<&str>) -> Result<Vec<CollectionStrategyRow>> {
    let collector_name = collector_name.map(|s| format!("%{}%", s));
    let strategy_type = strategy_type.map(|s| s.to_string());
    let status = status.map(|s| s.to_string());

    let mut sql = String::from("SELECT id, collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at FROM collection_strategy WHERE 1=1");
    if collector_name.is_some() { sql.push_str(" AND collector_name LIKE ?"); }
    if strategy_type.is_some() { sql.push_str(" AND strategy_type = ?"); }
    if status.is_some() { sql.push_str(" AND status = ?"); }
    sql.push_str(" ORDER BY id DESC");

    let mut query = sqlx::query_as::<_, CollectionStrategyRow>(&sql);
    if let Some(ref v) = collector_name { query = query.bind(v); }
    if let Some(ref v) = strategy_type { query = query.bind(v); }
    if let Some(ref v) = status { query = query.bind(v); }
    query.fetch_all(&self.pool).await
}
```

- [ ] **Step 5: Add `update_strategy` method**

```rust
pub async fn update_strategy(&self, id: i64, req: &CollectionStrategyUpdateRequest) -> Result<bool> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut sql = String::from("UPDATE collection_strategy SET updated_at = ?");
    let mut params: Vec<Box<dyn sqlx::Encode<'_, sqlx::Sqlite> + Send + Sync>> = Vec::new();
    params.push(Box::new(now));

    if let Some(v) = &req.cron_expression {
        sql.push_str(", cron_expression = ?");
        params.push(Box::new(v.clone()));
    }
    if let Some(v) = req.collect_interval {
        sql.push_str(", collect_interval = ?");
        params.push(Box::new(v));
    }
    if let Some(v) = req.data_interval {
        sql.push_str(", data_interval = ?");
        params.push(Box::new(v));
    }
    if let Some(v) = &req.data_start_time {
        sql.push_str(", data_start_time = ?");
        params.push(Box::new(v.clone()));
    }
    if let Some(v) = &req.data_end_time {
        sql.push_str(", data_end_time = ?");
        params.push(Box::new(v.clone()));
    }
    if let Some(v) = &req.execute_time {
        sql.push_str(", execute_time = ?");
        params.push(Box::new(v.clone()));
    }
    if let Some(v) = &req.agent_ids {
        sql.push_str(", agent_ids = ?");
        params.push(Box::new(v.clone()));
    }
    if let Some(v) = &req.status {
        sql.push_str(", status = ?");
        params.push(Box::new(v.clone()));
    }
    sql.push_str(" WHERE id = ?");
    params.push(Box::new(id));

    let mut query = sqlx::query(&sql);
    for p in params {
        query = query.bind(p);
    }
    let result = query.execute(&self.pool).await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 6: Add `batch_suspend` and `batch_activate` methods**

```rust
pub async fn batch_suspend(&self, ids: &[i64]) -> Result<usize> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut count = 0;
    for id in ids {
        let r = sqlx::query("UPDATE collection_strategy SET status = '挂起', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        count += r.rows_affected() as usize;
    }
    Ok(count)
}

pub async fn batch_activate(&self, ids: &[i64]) -> Result<usize> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut count = 0;
    for id in ids {
        let r = sqlx::query("UPDATE collection_strategy SET status = '可用', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        count += r.rows_affected() as usize;
    }
    Ok(count)
}
```

- [ ] **Step 7: Build and test**

Run: `cargo test --lib`
Expected: All tests pass (no new tests added yet, existing 46 pass)

- [ ] **Step 8: Commit**

```bash
git add src/core/db.rs
git commit -m "feat: add collection strategy DB schema and CRUD"
```

### Task 3: Backend — HTTP Handlers

**Files:**
- Modify: `src/core/server.rs`

**Interfaces:**
- Consumes: `CoreDb` methods from Task 2
- Produces: 7 new routes under `/api/strategies/*`

- [ ] **Step 1: Add routes in `CoreServer::new()` after existing `/api/data-collector-units/*` routes**

```rust
.route("/api/strategies/next-id", post(next_strategy_id))
.route("/api/strategies", post(create_strategies))
.route("/api/strategies", get(list_strategies))
.route("/api/strategies/:id", get(get_strategy))
.route("/api/strategies/:id", put(update_strategy))
.route("/api/strategies/batch-suspend", post(batch_suspend))
.route("/api/strategies/batch-activate", post(batch_activate))
```

Note: Order matters — `batch-suspend` and `batch-activate` must be registered BEFORE `/:id` to avoid matching `batch-suspend` as an `:id`.

- [ ] **Step 2: Add handler functions (place after existing data_collector_unit handlers)**

```rust
async fn next_strategy_id(
    State(state): State<CoreState>,
) -> Response {
    match state.db.next_strategy_id().await {
        Ok(id) => ok_response(serde_json::json!({ "id": id }), "OK").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn create_strategies(
    State(state): State<CoreState>,
    Json(req): Json<CollectionStrategyCreateRequest>,
) -> Response {
    if req.table_names.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "table_names 不能为空").into_response();
    }
    if !["immediate", "periodic"].contains(&req.strategy_type.as_str()) {
        return err_response(StatusCode::BAD_REQUEST, "strategy_type 必须是 immediate 或 periodic").into_response();
    }
    match state.db.create_strategies(&req).await {
        Ok(rows) => ok_response(rows, "创建成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn list_strategies(
    State(state): State<CoreState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let collector_name = params.get("collector_name").map(|s| s.as_str());
    let strategy_type = params.get("type").map(|s| s.as_str());
    let status = params.get("status").map(|s| s.as_str());
    match state.db.list_strategies(collector_name, strategy_type, status).await {
        Ok(rows) => ok_response(rows, "获取策略列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn get_strategy(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
) -> Response {
    match state.db.get_strategy(id).await {
        Ok(Some(row)) => ok_response(row, "OK").into_response(),
        Ok(None) => err_response(StatusCode::NOT_FOUND, "策略不存在").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn update_strategy(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
    Json(req): Json<CollectionStrategyUpdateRequest>,
) -> Response {
    match state.db.update_strategy(id, &req).await {
        Ok(true) => ok_response(serde_json::json!({}), "更新成功").into_response(),
        Ok(false) => err_response(StatusCode::NOT_FOUND, "策略不存在").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn batch_suspend(
    State(state): State<CoreState>,
    Json(req): Json<BatchStatusRequest>,
) -> Response {
    if req.ids.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "ids 不能为空").into_response();
    }
    match state.db.batch_suspend(&req.ids).await {
        Ok(count) => ok_response(serde_json::json!({ "affected": count }), &format!("已挂起 {count} 条")).into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}

async fn batch_activate(
    State(state): State<CoreState>,
    Json(req): Json<BatchStatusRequest>,
) -> Response {
    if req.ids.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "ids 不能为空").into_response();
    }
    match state.db.batch_activate(&req.ids).await {
        Ok(count) => ok_response(serde_json::json!({ "affected": count }), &format!("已激活 {count} 条")).into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    }
}
```

- [ ] **Step 3: Add required imports at top of server.rs**

Add these to existing imports:
```rust
use std::collections::HashMap;
use axum::extract::Query;
use crate::core_agent_api::{CollectionStrategyCreateRequest, CollectionStrategyUpdateRequest, BatchStatusRequest};
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add src/core/server.rs
git commit -m "feat: add collection strategy HTTP endpoints"
```

### Task 4: Frontend — Types, API, Hooks

**Files:**
- Modify: `pm-admin/src/types/api.ts`
- Create: `pm-admin/src/api/strategies.ts`
- Modify: `pm-admin/src/api/hooks.ts`

- [ ] **Step 1: Add TypeScript types to `types/api.ts`**

```typescript
export interface CollectionStrategy {
  id: number;
  collector_name: string;
  collector_id: number;
  table_name: string;
  status: string;
  cron_expression: string;
  collect_interval: number;
  data_interval: number;
  data_start_time: string | null;
  data_end_time: string | null;
  execute_time: string | null;
  agent_ids: string[];
  strategy_type: string;
  created_at: string;
  updated_at: string;
}

export interface CollectionStrategyCreateRequest {
  collector_id: number;
  collector_name: string;
  table_names: string[];
  cron_expression?: string;
  collect_interval: number;
  data_interval: number;
  data_start_time?: string;
  data_end_time?: string;
  execute_time?: string;
  agent_ids: string;
  strategy_type: string;
}

export interface CollectionStrategyUpdateRequest {
  cron_expression?: string;
  collect_interval?: number;
  data_interval?: number;
  data_start_time?: string;
  data_end_time?: string;
  execute_time?: string;
  agent_ids?: string;
  status?: string;
}

export interface BatchStatusRequest {
  ids: number[];
}
```

- [ ] **Step 2: Create `pm-admin/src/api/strategies.ts`**

```typescript
import http from '../http';
import type {
  CollectionStrategy,
  CollectionStrategyCreateRequest,
  CollectionStrategyUpdateRequest,
} from '../types/api';

export const strategyApi = {
  nextId: () => http.post<{ id: number }>('/api/strategies/next-id', {}),

  list: (params?: { collector_name?: string; type?: string; status?: string }) =>
    http.get<CollectionStrategy[]>('/api/strategies', { params }),

  get: (id: number) => http.get<CollectionStrategy>(`/api/strategies/${id}`),

  create: (data: CollectionStrategyCreateRequest) =>
    http.post<CollectionStrategy[]>('/api/strategies', data),

  update: (id: number, data: CollectionStrategyUpdateRequest) =>
    http.put<Record<string, never>>(`/api/strategies/${id}`, data),

  batchSuspend: (ids: number[]) =>
    http.post<{ affected: number }>('/api/strategies/batch-suspend', { ids }),

  batchActivate: (ids: number[]) =>
    http.post<{ affected: number }>('/api/strategies/batch-activate', { ids }),
};
```

- [ ] **Step 3: Add hooks to `hooks.ts`**

Check existing hooks pattern and add (note: API functions already unwrap `.data`, hooks do NOT add extra `.then(r => r.data)`):

```typescript
import { strategyApi } from './strategies';
import { useQueryClient } from '@tanstack/react-query';
import type { CollectionStrategyCreateRequest, CollectionStrategyUpdateRequest } from '../types/api';

export const useStrategies = (params?: { collector_name?: string; type?: string; status?: string }) =>
  useQuery({
    queryKey: ['strategies', params],
    queryFn: () => strategyApi.list(params),
    refetchInterval: 30_000,
  });

export const useStrategy = (id: number | null) =>
  useQuery({
    queryKey: ['strategy', id],
    queryFn: () => strategyApi.get(id!),
    enabled: id !== null,
  });

export const useCreateStrategies = () => {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CollectionStrategyCreateRequest) => strategyApi.create(data),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['strategies'] }); },
  });
};

export const useUpdateStrategy = () => {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: number; data: CollectionStrategyUpdateRequest }) =>
      strategyApi.update(id, data),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['strategies'] }); },
  });
};

export const useBatchSuspend = () => {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (ids: number[]) => strategyApi.batchSuspend(ids),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['strategies'] }); },
  });
};

export const useBatchActivate = () => {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (ids: number[]) => strategyApi.batchActivate(ids),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['strategies'] }); },
  });
};
```

- [ ] **Step 4: Build**

Run: `source ~/.nvm/nvm.sh && nvm use 22 && npm run build`
Expected: Compiles without errors

- [ ] **Step 5: Commit**

```bash
git add pm-admin/src/types/api.ts pm-admin/src/api/strategies.ts pm-admin/src/api/hooks.ts
git commit -m "feat: add collection strategy frontend types, API, and hooks"
```

### Task 5: Frontend — Strategy List Page

**Files:**
- Rewrite: `pm-admin/src/pages/StrategyDispatch/StrategyInfo.tsx`
- Modify: `pm-admin/src/App.tsx` (routes already exist from earlier sidebar change)

- [ ] **Step 1: Write the full list page**

```tsx
import { useCallback, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Table, Card, Button, message, Popconfirm, Tag, Checkbox, Space, Empty } from 'antd';
import { PlusOutlined, EditOutlined, PauseCircleOutlined, PlayCircleOutlined } from '@ant-design/icons';
import { useStrategies, useBatchSuspend, useBatchActivate } from '../../api/hooks';
import type { CollectionStrategy } from '../../types/api';
import type { ColumnsType } from 'antd/es/table';

export default function StrategyInfoPage() {
  const navigate = useNavigate();
  const { data: strategies, isLoading, refetch } = useStrategies();
  const suspendMutation = useBatchSuspend();
  const activateMutation = useBatchActivate();
  const [selectedIds, setSelectedIds] = useState<number[]>([]);

  const handleBatchSuspend = useCallback(async () => {
    if (selectedIds.length === 0) return;
    try {
      await suspendMutation.mutateAsync(selectedIds);
      message.success(`已挂起 ${selectedIds.length} 条`);
      setSelectedIds([]);
      refetch();
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [selectedIds, suspendMutation, refetch]);

  const handleBatchActivate = useCallback(async () => {
    if (selectedIds.length === 0) return;
    try {
      await activateMutation.mutateAsync(selectedIds);
      message.success(`已激活 ${selectedIds.length} 条`);
      setSelectedIds([]);
      refetch();
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [selectedIds, activateMutation, refetch]);

  const columns: ColumnsType<CollectionStrategy> = [
    { title: '策略Id', dataIndex: 'id', key: 'id', width: 70 },
    { title: '采集单元名称', dataIndex: 'collector_name', key: 'collector_name' },
    { title: '表名', dataIndex: 'table_name', key: 'table_name' },
    {
      title: '类型', dataIndex: 'strategy_type', key: 'strategy_type', width: 80,
      render: (v: string) => v === 'immediate' ? <Tag color="blue">及时</Tag> : <Tag color="green">周期</Tag>,
    },
    { title: 'Crontab', dataIndex: 'cron_expression', key: 'cron_expression' },
    {
      title: '采集机', key: 'agents', width: 160,
      render: (_: unknown, r: CollectionStrategy) => r.agent_ids.join(', '),
    },
    {
      title: '状态', dataIndex: 'status', key: 'status', width: 80,
      render: (v: string) => v === '可用'
        ? <Tag color="success">可用</Tag>
        : <Tag color="default">挂起</Tag>,
    },
    {
      title: '操作', key: 'action', width: 140,
      render: (_: unknown, record: CollectionStrategy) => (
        <span onClick={e => e.stopPropagation()}>
          <Button type="link" size="small" icon={<EditOutlined />} aria-label="编辑"
            onClick={() => navigate(`/strategy-dispatch/${record.strategy_type}/${record.id}/edit`)} />
          {record.status === '可用' ? (
            <Popconfirm title="确认挂起?" onConfirm={async () => {
              await suspendMutation.mutateAsync([record.id]);
              message.success('已挂起');
              refetch();
            }}>
              <Button type="link" size="small" icon={<PauseCircleOutlined />} aria-label="挂起" />
            </Popconfirm>
          ) : (
            <Popconfirm title="确认激活?" onConfirm={async () => {
              await activateMutation.mutateAsync([record.id]);
              message.success('已激活');
              refetch();
            }}>
              <Button type="link" size="small" icon={<PlayCircleOutlined />} aria-label="激活" />
            </Popconfirm>
          )}
        </span>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <h2>采集策略信息</h2>
        <p>查看和管理所有采集策略</p>
      </div>

      <Card
        className="content-card"
        styles={{ body: { padding: 0 } }}
        extra={
          <Space>
            {selectedIds.length > 0 && (
              <>
                <Button icon={<PauseCircleOutlined />} onClick={handleBatchSuspend} loading={suspendMutation.isPending}>
                  批量挂起 ({selectedIds.length})
                </Button>
                <Button icon={<PlayCircleOutlined />} onClick={handleBatchActivate} loading={activateMutation.isPending}>
                  批量激活 ({selectedIds.length})
                </Button>
              </>
            )}
          </Space>
        }
      >
        <Table<CollectionStrategy>
          className="data-table"
          rowKey="id"
          dataSource={strategies}
          columns={columns}
          loading={isLoading}
          pagination={false}
          size="small"
          scroll={{ x: 'max-content' }}
          locale={{ emptyText: <Empty description="暂无策略数据" /> }}
          rowSelection={{
            selectedRowKeys: selectedIds,
            onChange: (keys) => setSelectedIds(keys as number[]),
          }}
          onRow={(record) => ({
            onClick: () => navigate(`/strategy-dispatch/${record.strategy_type}/${record.id}/edit`),
            style: { cursor: 'pointer' },
          })}
        />
      </Card>
    </div>
  );
}
```

Note: Add `import type { ColumnsType } from 'antd/es/table';` — this is needed for the column type.

- [ ] **Step 2: Build**

Run: `source ~/.nvm/nvm.sh && nvm use 22 && npm run build`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add pm-admin/src/pages/StrategyDispatch/StrategyInfo.tsx
git commit -m "feat: add collection strategy list page with batch ops"
```

### Task 6: Frontend — Immediate & Periodic Strategy Forms

**Files:**
- Rewrite: `pm-admin/src/pages/StrategyDispatch/ImmediateStrategy.tsx`
- Rewrite: `pm-admin/src/pages/StrategyDispatch/PeriodicStrategy.tsx`
- Modify: `pm-admin/src/App.tsx` (add edit routes)

- [ ] **Step 1: Write ImmediateStrategy form page**

```tsx
import { useEffect, useCallback, useMemo, useRef, useState } from 'react';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { Card, Form, Input, InputNumber, Select, Button, message, DatePicker, Spin } from 'antd';
import { SaveOutlined, ArrowLeftOutlined } from '@ant-design/icons';
import { useDataCollectorUnits, useCreateStrategies, useStrategy, useUpdateStrategy } from '../../api/hooks';
import type { CollectionStrategyCreateRequest, CollectionStrategyUpdateRequest } from '../../types/api';
import dayjs from 'dayjs';

export default function ImmediateStrategyPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const isNew = location.pathname.endsWith('/immediate');
  const editId = isNew ? null : (id ? Number(id) : null);

  const { data: units } = useDataCollectorUnits();
  const { data: editData } = useStrategy(editId);
  const createMutation = useCreateStrategies();
  const updateMutation = useUpdateStrategy();

  const [form] = Form.useForm();
  const watchedCollectorId = Form.useWatch('collector_id', form);

  const selectedUnit = useMemo(() => {
    if (!watchedCollectorId || !units) return null;
    return units.find(u => u.id === watchedCollectorId) || null;
  }, [watchedCollectorId, units]);

  const availableTables = selectedUnit?.table_names ?? [];

  // Auto-fill when unit selected
  useEffect(() => {
    if (selectedUnit) {
      form.setFieldsValue({
        collector_name: selectedUnit.unit_name,
        collect_interval: selectedUnit.collect_interval,
        data_interval: selectedUnit.data_interval,
        agent_ids: selectedUnit.agent_ids,
      });
    }
  }, [selectedUnit, form]);

  // Load edit data
  useEffect(() => {
    if (editData) {
      form.setFieldsValue({
        ...editData,
        data_start_time: editData.data_start_time ? dayjs(editData.data_start_time) : undefined,
        data_end_time: editData.data_end_time ? dayjs(editData.data_end_time) : undefined,
        execute_time: editData.execute_time ? dayjs(editData.execute_time) : undefined,
      });
    }
  }, [editData, form]);

  const handleSave = useCallback(async () => {
    try {
      const values = await form.validateFields();
      if (isNew) {
        const data: CollectionStrategyCreateRequest = {
          collector_id: values.collector_id,
          collector_name: values.collector_name,
          table_names: values.table_names || [],
          collect_interval: values.collect_interval,
          data_interval: values.data_interval,
          data_start_time: values.data_start_time?.format('YYYY-MM-DD HH:mm:ss'),
          data_end_time: values.data_end_time?.format('YYYY-MM-DD HH:mm:ss'),
          execute_time: values.execute_time?.format('YYYY-MM-DD HH:mm:ss'),
          agent_ids: JSON.stringify(values.agent_ids || []),
          strategy_type: 'immediate',
        };
        await createMutation.mutateAsync(data);
        message.success('创建成功，任务已执行');
      } else if (editId) {
        const data: CollectionStrategyUpdateRequest = {
          data_start_time: values.data_start_time?.format('YYYY-MM-DD HH:mm:ss'),
          data_end_time: values.data_end_time?.format('YYYY-MM-DD HH:mm:ss'),
          execute_time: values.execute_time?.format('YYYY-MM-DD HH:mm:ss'),
          agent_ids: JSON.stringify(values.agent_ids || []),
        };
        await updateMutation.mutateAsync({ id: editId, data });
        message.success('更新成功');
      }
      navigate('/strategy-dispatch/info');
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [form, isNew, editId, createMutation, updateMutation, navigate]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <div style={{ flexShrink: 0, display: 'flex', alignItems: 'center', justifyContent: 'space-between', paddingBottom: 16, marginBottom: 16, position: 'sticky', top: 0, zIndex: 10, background: 'var(--color-bg-layout)' }}>
        <div>
          <Button type="text" icon={<ArrowLeftOutlined />} aria-label="返回" onClick={() => navigate('/strategy-dispatch/info')} style={{ marginRight: 8 }} />
          <h2 style={{ display: 'inline' }}>{isNew ? '新建及时采集策略' : '编辑及时采集策略'}</h2>
        </div>
        <div>
          <Button onClick={() => navigate('/strategy-dispatch/info')} style={{ marginRight: 8 }}>取消</Button>
          <Button type="primary" icon={<SaveOutlined />} onClick={handleSave} loading={createMutation.isPending || updateMutation.isPending}>保存</Button>
        </div>
      </div>

      <div style={{ flex: 1, overflowY: 'auto' }}>
        <Card className="content-card">
          <Form form={form} layout="vertical">
            <Form.Item name="collector_id" label="采集单元" rules={[{ required: true }]}>
              <Select showSearch placeholder="搜索并选择采集单元" filterOption={(input, option) => (option?.label as string ?? '').includes(input)}
                options={(units ?? []).map(u => ({ label: u.unit_name, value: u.id }))} disabled={!isNew} />
            </Form.Item>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ flex: 1 }}><Form.Item name="collector_name" label="采集单元名称"><Input disabled /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="collect_interval" label="采集周期(秒)"><InputNumber disabled style={{ width: '100%' }} /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="data_interval" label="数据周期(秒)"><InputNumber disabled style={{ width: '100%' }} /></Form.Item></div>
            </div>
            <Form.Item name="table_names" label="指标组(表名)" rules={[{ required: true }]}>
              <Select mode="multiple" placeholder="选择表名" options={availableTables.map(t => ({ label: t, value: t }))} disabled={!isNew} />
            </Form.Item>
            <Form.Item name="agent_ids" label="采集机" rules={[{ required: true }]}>
              <Select mode="multiple" placeholder="选择采集机" options={(units ?? []).find(u => u.id === watchedCollectorId)?.agent_ids.map(a => ({ label: a, value: a })) ?? []} />
            </Form.Item>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ flex: 1 }}><Form.Item name="data_start_time" label="数据开始时间"><DatePicker showTime style={{ width: '100%' }} /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="data_end_time" label="数据结束时间"><DatePicker showTime style={{ width: '100%' }} /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="execute_time" label="执行时间"><DatePicker showTime style={{ width: '100%' }} /></Form.Item></div>
            </div>
          </Form>
        </Card>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Write PeriodicStrategy form page**

```tsx
import { useEffect, useCallback, useMemo, useRef, useState } from 'react';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { Card, Form, Input, InputNumber, Select, Button, message } from 'antd';
import { SaveOutlined, ArrowLeftOutlined } from '@ant-design/icons';
import { useDataCollectorUnits, useCreateStrategies, useStrategy, useUpdateStrategy } from '../../api/hooks';
import type { CollectionStrategyCreateRequest, CollectionStrategyUpdateRequest } from '../../types/api';

export default function PeriodicStrategyPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const isNew = location.pathname.endsWith('/periodic');
  const editId = isNew ? null : (id ? Number(id) : null);

  const { data: units } = useDataCollectorUnits();
  const { data: editData } = useStrategy(editId);
  const createMutation = useCreateStrategies();
  const updateMutation = useUpdateStrategy();

  const [form] = Form.useForm();
  const watchedCollectorId = Form.useWatch('collector_id', form);

  const selectedUnit = useMemo(() => {
    if (!watchedCollectorId || !units) return null;
    return units.find(u => u.id === watchedCollectorId) || null;
  }, [watchedCollectorId, units]);

  const availableTables = selectedUnit?.table_names ?? [];

  useEffect(() => {
    if (selectedUnit) {
      form.setFieldsValue({
        collector_name: selectedUnit.unit_name,
        collect_interval: selectedUnit.collect_interval,
        data_interval: selectedUnit.data_interval,
        agent_ids: selectedUnit.agent_ids,
      });
    }
  }, [selectedUnit, form]);

  useEffect(() => {
    if (editData) {
      form.setFieldsValue(editData);
    }
  }, [editData, form]);

  const handleSave = useCallback(async () => {
    try {
      const values = await form.validateFields();
      if (isNew) {
        const data: CollectionStrategyCreateRequest = {
          collector_id: values.collector_id,
          collector_name: values.collector_name,
          table_names: values.table_names || [],
          cron_expression: values.cron_expression,
          collect_interval: values.collect_interval,
          data_interval: values.data_interval,
          agent_ids: JSON.stringify(values.agent_ids || []),
          strategy_type: 'periodic',
        };
        await createMutation.mutateAsync(data);
        message.success('创建成功');
      } else if (editId) {
        const data: CollectionStrategyUpdateRequest = {
          cron_expression: values.cron_expression,
          collect_interval: values.collect_interval,
          data_interval: values.data_interval,
          agent_ids: JSON.stringify(values.agent_ids || []),
          status: values.status,
        };
        await updateMutation.mutateAsync({ id: editId, data });
        message.success('更新成功');
      }
      navigate('/strategy-dispatch/info');
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [form, isNew, editId, createMutation, updateMutation, navigate]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <div style={{ flexShrink: 0, display: 'flex', alignItems: 'center', justifyContent: 'space-between', paddingBottom: 16, marginBottom: 16, position: 'sticky', top: 0, zIndex: 10, background: 'var(--color-bg-layout)' }}>
        <div>
          <Button type="text" icon={<ArrowLeftOutlined />} aria-label="返回" onClick={() => navigate('/strategy-dispatch/info')} style={{ marginRight: 8 }} />
          <h2 style={{ display: 'inline' }}>{isNew ? '新建周期性采集策略' : '编辑周期性采集策略'}</h2>
        </div>
        <div>
          <Button onClick={() => navigate('/strategy-dispatch/info')} style={{ marginRight: 8 }}>取消</Button>
          <Button type="primary" icon={<SaveOutlined />} onClick={handleSave} loading={createMutation.isPending || updateMutation.isPending}>保存</Button>
        </div>
      </div>

      <div style={{ flex: 1, overflowY: 'auto' }}>
        <Card className="content-card">
          <Form form={form} layout="vertical">
            <Form.Item name="collector_id" label="采集单元" rules={[{ required: true }]}>
              <Select showSearch placeholder="搜索并选择采集单元" filterOption={(input, option) => (option?.label as string ?? '').includes(input)}
                options={(units ?? []).map(u => ({ label: u.unit_name, value: u.id }))} disabled={!isNew} />
            </Form.Item>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ flex: 1 }}><Form.Item name="collector_name" label="采集单元名称"><Input disabled /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="collect_interval" label="采集周期(秒)"><InputNumber disabled style={{ width: '100%' }} /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="data_interval" label="数据周期(秒)"><InputNumber disabled style={{ width: '100%' }} /></Form.Item></div>
            </div>
            <Form.Item name="table_names" label="指标组(表名)" rules={[{ required: true }]}>
              <Select mode="multiple" placeholder="选择表名" options={availableTables.map(t => ({ label: t, value: t }))} disabled={!isNew} />
            </Form.Item>
            <Form.Item name="cron_expression" label="采集时间(Crontab)" rules={[{ required: true }]}>
              <Input placeholder="0 0 * * *" />
            </Form.Item>
            <Form.Item name="agent_ids" label="采集机" rules={[{ required: true }]}>
              <Select mode="multiple" placeholder="选择采集机" options={(units ?? []).find(u => u.id === watchedCollectorId)?.agent_ids.map(a => ({ label: a, value: a })) ?? []} />
            </Form.Item>
            {!isNew && (
              <Form.Item name="status" label="状态">
                <Select options={[{ label: '可用', value: '可用' }, { label: '挂起', value: '挂起' }]} />
              </Form.Item>
            )}
          </Form>
        </Card>
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Add edit routes to App.tsx**

```tsx
<Route path="/strategy-dispatch/immediate/:id/edit" element={<ImmediateStrategyPage />} />
<Route path="/strategy-dispatch/periodic/:id/edit" element={<PeriodicStrategyPage />} />
```

Place these after the existing strategy-dispatch routes.

- [ ] **Step 4: Build**

Run: `source ~/.nvm/nvm.sh && nvm use 22 && npm run build`
Expected: Compiles without errors

- [ ] **Step 5: Commit**

```bash
git add pm-admin/src/pages/StrategyDispatch/ImmediateStrategy.tsx pm-admin/src/pages/StrategyDispatch/PeriodicStrategy.tsx pm-admin/src/App.tsx
git commit -m "feat: add immediate and periodic strategy forms"
```

### Task 7: Backend — Test that CRUD works end-to-end

- [ ] **Step 1: Add integration test to `db.rs` tests module**

```rust
#[tokio::test]
async fn collection_strategy_crud() {
    let db = db().await;

    // Need a collector unit + config for validation
    db.insert_config_snapshot_meta("v_strat", "sha256:strat", "v_strat", 1, "cfg-strat", &["t1".to_string(), "t2".to_string()]).await.unwrap();
    db.activate_config_snapshot("v_strat").await.unwrap();
    // Insert a data_collector_unit so we can reference it
    let save = DataCollectorUnitSaveRequest {
        unit_name: "strat-unit".to_string(),
        config_name: "cfg-strat".to_string(),
        table_names: "[\"t1\",\"t2\"]".to_string(),
        agent_ids: "[]".to_string(),
        data_interval_seconds: Some(900),
        collector_interval: Some(900),
        task_timeout_seconds: Some(3600),
        source_type: Some("sftp".to_string()),
        file_encoding: Some("UTF-8".to_string()),
        remote_pattern: Some("/path".to_string()),
        host: Some("host".to_string()),
        port: Some(22),
        username: Some("u".to_string()),
        password: Some("p".to_string()),
        connect_retry: Some(3),
        download_retry: Some(3),
        download_parallel: Some(1),
        retry_interval_secs: Some(30),
        connect_timeout_secs: Some(30),
        read_timeout_secs: Some(300),
        cache_retention_days: Some(7),
    };
    db.upsert_data_collector_unit(1, &save).await.unwrap();

    // Create strategies
    let req = CollectionStrategyCreateRequest {
        collector_id: 1,
        collector_name: "strat-unit".to_string(),
        table_names: vec!["t1".to_string(), "t2".to_string()],
        cron_expression: Some("0 0 * * *".to_string()),
        collect_interval: 900,
        data_interval: 900,
        data_start_time: None,
        data_end_time: None,
        execute_time: None,
        agent_ids: "[]".to_string(),
        strategy_type: "periodic".to_string(),
    };
    let rows = db.create_strategies(&req).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].table_name, "t1");
    assert_eq!(rows[0].status, "可用");

    // List
    let list = db.list_strategies(None, None, None).await.unwrap();
    assert_eq!(list.len(), 2);

    // Get
    let row = db.get_strategy(rows[0].id).await.unwrap().unwrap();
    assert_eq!(row.table_name, "t1");

    // Update
    let update = CollectionStrategyUpdateRequest {
        cron_expression: Some("0 */2 * * *".to_string()),
        collect_interval: None,
        data_interval: None,
        data_start_time: None,
        data_end_time: None,
        execute_time: None,
        agent_ids: None,
        status: Some("挂起".to_string()),
    };
    let ok = db.update_strategy(rows[0].id, &update).await.unwrap();
    assert!(ok);
    let updated = db.get_strategy(rows[0].id).await.unwrap().unwrap();
    assert_eq!(updated.status, "挂起");
    assert_eq!(updated.cron_expression, "0 */2 * * *");

    // Batch suspend
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    db.batch_suspend(&ids).await.unwrap();
    assert_eq!(db.get_strategy(rows[0].id).await.unwrap().unwrap().status, "挂起");
    assert_eq!(db.get_strategy(rows[1].id).await.unwrap().unwrap().status, "挂起");

    // Batch activate
    db.batch_activate(&ids).await.unwrap();
    assert_eq!(db.get_strategy(rows[0].id).await.unwrap().unwrap().status, "可用");
    assert_eq!(db.get_strategy(rows[1].id).await.unwrap().unwrap().status, "可用");
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --lib collection_strategy_crud -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/core/db.rs
git commit -m "test: add collection strategy CRUD test"
```
