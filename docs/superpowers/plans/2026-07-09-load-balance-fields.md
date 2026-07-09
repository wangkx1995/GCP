# 负载均衡页面新增字段 Implementation Plan

**Goal:** Add 3 fields to the load balancing page: agent_power (采集能力), new_task_count (总新任务数), active_task_count (采集机任务数).

**Architecture:** Backend: subquery counts in `list_agent_status()` SQL. Frontend: new columns in `StatusPage.tsx`.

**Tech Stack:** Rust (sqlx), TypeScript (React/Ant Design)

## Global Constraints

- Field names must be consistent across Rust/TS/Frontend: `agent_power`, `new_task_count`, `active_task_count`
- `agent_power` is `Option<f64>` (may be NULL in DB)
- Task counts are `i64` in Rust, `number` in TS, derived from subqueries

---

### Task 1: Backend — types + SQL

**Files:**
- Modify: `src/core_agent_api.rs` — `AgentStatusRow`
- Modify: `src/core/db.rs` — `list_agent_status()` SQL

Add 3 fields to `AgentStatusRow`:

```rust
pub agent_power: Option<f64>,
pub new_task_count: i64,
pub active_task_count: i64,
```

Replace the SQL in `list_agent_status()` with:

```sql
SELECT
  ast.*,
  ai.agent_name,
  ai.agent_alias,
  ai.agent_power,
  COALESCE((SELECT COUNT(*) FROM collect_tasks
    WHERE assigned_agent_id = ast.agent_id AND status IN ('CREATED', 'DISPATCHING')), 0) AS new_task_count,
  COALESCE((SELECT COUNT(*) FROM collect_tasks
    WHERE assigned_agent_id = ast.agent_id AND status IN ('ACCEPTED', 'RUNNING')), 0) AS active_task_count
FROM agent_status ast
JOIN agent_info ai ON ai.agent_id = ast.agent_id
WHERE ai.agent_isuse_flag = 1
ORDER BY ast.heartbeat_time DESC
```

Run `cargo test --lib` to verify.

### Task 2: Frontend — types + columns

**Files:**
- Modify: `pm-admin/src/types/api.ts`
- Modify: `pm-admin/src/pages/Agents/StatusPage.tsx`

Add to `AgentStatusRow` TS interface:

```typescript
agent_power?: number;
new_task_count: number;
active_task_count: number;
```

Add columns after "状态" column in `StatusPage.tsx`:

```typescript
{ title: '采集能力', dataIndex: 'agent_power', width: 90, render: (v?: number) => v != null ? v.toFixed(1) : '—' },
{ title: '总新任务数', dataIndex: 'new_task_count', width: 100 },
{ title: '采集机任务数', dataIndex: 'active_task_count', width: 105 },
```

Run `npm run build` to verify.
