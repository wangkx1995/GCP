# Agent Management Design

> **Goal:** Replace the simple 1-table `agents` schema with a 4-table agent management system covering agent info, real-time status, status history, and agent groups.

**Architecture:** Extend existing `AgentRegisterRequest` and `AgentHeartbeatRequest` messages with new resource/load fields. Core DB schema uses 4 tables (agent_info, agent_status, agent_status_his, agent_group). New HTTP API endpoints for management. Frontend adds sub-menu with 4 pages: 采集机信息, 实时状态, 状态历史, 采集机组.

**Tech Stack:** Rust (sqlx + axum), SQLite, React + Ant Design + Recharts

---

## Global Constraints

- agent_id = `ip_to_u32(agent_ip) * 65536 + port` as INTEGER PRIMARY KEY
- Registration inserts into `agent_info` + upserts `agent_status`
- Heartbeat updates `agent_status` + inserts `agent_status_his`
- TCP timeout sets `agent_status.status = 'OFFLINE'`
- Task dispatch filters `agent_status.status = 'ONLINE'` AND `agent_info.agent_isuse_flag = 1`
- Old `agents` table is dropped and replaced
- Frontend uses sub-menu under existing "采集机管理" menu item

---

## Database Schema

### agent_info

```sql
CREATE TABLE IF NOT EXISTS agent_info (
    agent_id            INTEGER PRIMARY KEY,
    agent_name          TEXT NOT NULL,
    agent_ip            TEXT NOT NULL,
    port                INTEGER NOT NULL,
    version             TEXT NOT NULL,
    cpu_total           TEXT,
    memory_total        REAL,
    disk_total          REAL,
    heartbeat_interval  INTEGER,
    time_stamp          TEXT DEFAULT (datetime('now','localtime')),
    description         TEXT,
    max_thread_num      INTEGER,
    agent_isuse_flag    INTEGER NOT NULL DEFAULT 1,
    fact_memory_total   REAL,
    agent_alias         TEXT,
    is_core             INTEGER NOT NULL DEFAULT 0,
    agent_power         REAL DEFAULT 1.0,
    host_load_limit     REAL DEFAULT 90.0,
    registered_at       TEXT NOT NULL
);
```

### agent_status

```sql
CREATE TABLE IF NOT EXISTS agent_status (
    agent_id          INTEGER PRIMARY KEY,
    status            TEXT NOT NULL,
    cpu_load          REAL,
    memory_load       REAL,
    disk_load         REAL,
    heartbeat_time    TEXT NOT NULL,
    thread_num        INTEGER,
    description       TEXT
);
```

### agent_status_his

```sql
CREATE TABLE IF NOT EXISTS agent_status_his (
    agent_id          INTEGER NOT NULL,
    cpu_load          REAL,
    memory_load       REAL,
    disk_load         REAL,
    heartbeat_time    TEXT NOT NULL,
    thread_num        INTEGER,
    description       TEXT,
    insert_time       TEXT DEFAULT (datetime('now','localtime'))
);
CREATE INDEX IF NOT EXISTS idx_agent_status_his_agent_time
    ON agent_status_his(agent_id, heartbeat_time);
```

### agent_group

```sql
CREATE TABLE IF NOT EXISTS agent_group (
    group_id    INTEGER PRIMARY KEY AUTOINCREMENT,
    group_name  TEXT NOT NULL,
    agent_ids   TEXT DEFAULT '[]' NOT NULL,
    description TEXT,
    time_stamp  TEXT
);
```

---

## Agent Protocol Changes

### AgentRegisterRequest (extended)

Current fields (kept): `agent_id, agent_name, host, port, version, capabilities`

New fields:
```
cpu_total: Option<String>       — CPU model/count string
memory_total: Option<f64>       — total memory in MB
disk_total: Option<f64>         — total disk in GB
max_thread_num: Option<i32>     — max concurrent threads
fact_memory_total: Option<f64>  — actual usable memory in MB
heartbeat_interval: Option<i32> — heartbeat interval in seconds (default 10)
is_core: Option<bool>           — is core node (default false)
```

### AgentHeartbeatRequest (extended)

Current fields (kept): `status, running_task_ids, disk_free_bytes`

New fields:
```
cpu_load: Option<f64>    — CPU load percentage (0.0–100.0)
memory_load: Option<f64> — memory load percentage (0.0–100.0)
disk_load: Option<f64>   — disk load percentage (0.0–100.0)
thread_num: Option<i32>  — current active threads
```

---

## Backend Flow

### agent_id Computation

```rust
fn compute_agent_id(host: &str, port: u16) -> Result<i64> {
    let ip: std::net::Ipv4Addr = host.parse()?;
    let ip_u32 = u32::from_be_bytes(ip.octets()); // network byte order
    Ok((ip_u32 as i64) * 65536 + port as i64)
}
```

If `host` is not a valid IPv4 address (e.g., hostname), compute a negative ID from a hash:
`agent_id = -(abs(hash(host:port)) % i64::MAX)`. This guarantees agent_id is always non-zero and hostname-based agents get distinguishable negative IDs.

### Agent Registration (in `tcp_dispatch_loop`)

1. Core receives `AgentRegister` from agent TCP connection
2. Compute `agent_id = compute_agent_id(req.host, req.port)`
3. Override `req.agent_id = Some(agent_id.to_string())` for downstream use
4. `INSERT INTO agent_info(...) ON CONFLICT(agent_id) DO UPDATE SET ...`
5. `INSERT INTO agent_status(agent_id, status, heartbeat_time) VALUES(?, 'ONLINE', ?) ON CONFLICT(agent_id) DO UPDATE SET status='ONLINE', heartbeat_time=?`
6. Respond with `AgentRegisterAck { agent_id: agent_id.to_string(), ... }`

### Heartbeat (in `listener.rs`)

1. Core receives `Heartbeat` from agent
2. Update registry timestamp (existing behavior)
3. `UPDATE agent_status SET status='ONLINE', cpu_load=?, memory_load=?, disk_load=?, thread_num=?, heartbeat_time=? WHERE agent_id=?`
4. `INSERT INTO agent_status_his(agent_id, cpu_load, memory_load, disk_load, thread_num, heartbeat_time)`
5. Response: `HeartbeatAck`

### TCP Timeout (in `tcp_cleanup_loop`)

1. Registry detects agent heartbeat timeout
2. Unregister from registry (existing)
3. `UPDATE agent_status SET status='OFFLINE' WHERE agent_id=?`

### Task Dispatch (`select_online_agent`)

Current query selects ONLINE agent from `agents` table. New query returns `(agent_id: i64, agent_power: f64)`:

```sql
SELECT ai.agent_id, ai.agent_power
FROM agent_info ai
JOIN agent_status ast ON ast.agent_id = ai.agent_id
WHERE ast.status = 'ONLINE'
  AND ai.agent_isuse_flag = 1
ORDER BY ast.heartbeat_time DESC
LIMIT 1
```

---

## HTTP API Endpoints

### Agent Info
| Method | Path | Description |
|--------|------|-------------|
| GET | /api/agents | List all agents (agent_info + agent_status joined) |
| GET | /api/agents/:id | Single agent detail |
| PATCH | /api/agents/:id | Update agent (alias, isuse_flag, power, load_limit, etc.) |

### Agent Status
| Method | Path | Description |
|--------|------|-------------|
| GET | /api/agents/status | List all agents real-time status |
| GET | /api/agents/:id/status-history | Status history for a single agent |

### Agent Groups
| Method | Path | Description |
|--------|------|-------------|
| GET | /api/agent-groups | List groups |
| POST | /api/agent-groups | Create group |
| PUT | /api/agent-groups/:id | Update group |
| DELETE | /api/agent-groups/:id | Delete group |

---

## Frontend Structure

```
采集机管理 (sub-menu)
├── 采集机信息 → /agents
├── 实时状态   → /agents/status
├── 状态历史   → /agents/history
└── 采集机组   → /agent-groups
```

### 采集机信息 Page
Table columns: agent_id, agent_name, agent_alias, agent_ip, port, version, cpu_total, memory_total, disk_total, is_core, agent_power, host_load_limit, agent_isuse_flag (toggle switch), heartbeat_interval, description, registered_at, time_stamp

### 实时状态 Page
Table columns: agent_id, agent_name, status (ONLINE/OFFLINE badge), cpu_load, memory_load, disk_load, thread_num, heartbeat_time

Auto-refresh every 10s.

### 状态历史 Page
Agent selector dropdown + date range picker + line chart (Recharts) showing cpu_load, memory_load, disk_load over time.

### 采集机组 Page
CRUD table: group_id, group_name, agent_count, description, time_stamp. Create/Edit modal with agent multi-select.

---

## Implementation Order

1. **DB schema migration**: Drop old `agents` table, create 4 new tables
2. **Extend agent messages**: New fields in `AgentRegisterRequest` and `AgentHeartbeatRequest`
3. **Agent side**: Auto-detect and report system resources on register, load metrics on heartbeat
4. **Core registration handler**: Replace with new 2-table insert/upsert
5. **Core heartbeat handler**: Update agent_status + insert history
6. **TCP timeout handler**: Update status to OFFLINE
7. **select_online_agent**: New JOIN query with isuse_flag filter
8. **HTTP API**: New endpoints for agent info, status, groups
9. **Frontend**: Agent info page, real-time status page, history chart page, group CRUD

---

## Data Migration

Old `agents` table has 9 columns. Migration on first startup:

```sql
DROP TABLE IF EXISTS agents;
-- Create 4 new tables via init_schema()
```

No data migration needed — old table is dropped (no production data).
