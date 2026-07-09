# 负载均衡页面新增字段设计

## 背景

负载均衡页面（`/agents/status`）当前显示 7 列：别名、状态、CPU/内存/磁盘负载、线程数、最后心跳。需要在列表中加入采集能力、任务统计字段，便于管理员直观了解各采集机负载状况。

## 新增字段

### 数据来源

| 字段 | 数据来源 | 计算方式 |
|---|---|---|
| 采集能力 | `agent_info.agent_power` | 已有字段，直接透出 |
| 总新任务数 | `collect_tasks` | `COUNT(*) WHERE status IN ('CREATED', 'DISPATCHING') AND assigned_agent_id = ?` |
| 采集机任务数 | `collect_tasks` | `COUNT(*) WHERE status IN ('ACCEPTED', 'RUNNING') AND assigned_agent_id = ?` |

### 拆分明细

原 `count_active_tasks_by_agent()` 统计所有非终态任务，本次拆为两个独立查询：
- **新任务**（已分配未执行）：`CREATED`、`DISPATCHING`
- **活跃任务**（正在执行）：`ACCEPTED`、`RUNNING`

## 后端改动

### `AgentStatusRow`（Rust + TypeScript）

Rust `src/core_agent_api.rs`:
```rust
pub struct AgentStatusRow {
    // ... existing fields ...
    pub agent_power: Option<f64>,
    pub new_task_count: i64,
    pub active_task_count: i64,
}
```

TypeScript `pm-admin/src/types/api.ts`:
```typescript
export interface AgentStatusRow {
  // ... existing fields ...
  agent_power?: number;
  new_task_count: number;
  active_task_count: number;
}
```

### SQL 查询

`list_agent_status()` 改为带子查询的 JOIN：

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

## 前端改动

### 负载均衡页面列

在 `pm-admin/src/pages/Agents/StatusPage.tsx` 的 `columns` 中新增 3 列，跟在"状态"列之后：

```typescript
{ title: '采集能力', dataIndex: 'agent_power', width: 90, render: (v?: number) => v != null ? v.toFixed(1) : '—' },
{ title: '总新任务数', dataIndex: 'new_task_count', width: 100 },
{ title: '采集机任务数', dataIndex: 'active_task_count', width: 105 },
```

## 涉及文件

| 文件 | 改动 |
|---|---|
| `src/core_agent_api.rs` | `AgentStatusRow` 加 3 字段 |
| `src/core/db.rs` | `list_agent_status()` SQL 加子查询 |
| `pm-admin/src/types/api.ts` | `AgentStatusRow` 加 3 字段 |
| `pm-admin/src/pages/Agents/StatusPage.tsx` | columns 加 3 列 |
