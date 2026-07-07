# Agent Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the simple 1-table `agents` schema with a 4-table agent management system covering agent info, real-time status, status history, and agent groups.

**Architecture:** Extend existing `AgentRegisterRequest` and `AgentHeartbeatRequest` messages with new resource/load fields. Core DB schema uses 4 tables (agent_info, agent_status, agent_status_his, agent_group). New HTTP API endpoints for management. Frontend adds 4 sub-pages under 采集机管理.

**Tech Stack:** Rust (sqlx 0.8 + axum 0.7), SQLite, React 18 + Ant Design 5 + Recharts 2

## Global Constraints

- agent_id = `u32::from_be_bytes(ip.octets()) as i64 * 65536 + port as i64` as INTEGER PRIMARY KEY
- Registration upserts `agent_info` + upserts `agent_status` with status='ONLINE'
- Heartbeat updates `agent_status` + inserts `agent_status_his`
- TCP timeout (150s) sets `agent_status.status = 'OFFLINE'`
- Task dispatch filters `agent_status.status = 'ONLINE'` AND `agent_info.agent_isuse_flag = 1`
- Old `agents` table is dropped, 4 new tables created in `init_schema()`
- `AgentRegisterRequest` keeps all existing fields; new fields are `Option<>`
- `AgentHeartbeatRequest` keeps all existing fields; new fields are `Option<>`
- Frontend sub-menu under existing "采集机管理" menu item in Layout
- After Rust changes: `cargo test && cargo build --release && cp target/release/core test/core && cp target/release/agent test/agent`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/core/db.rs` | Schema init, CRUD: upsert_agent_info, upsert_agent_status, insert_status_his, update_agent, list_agents, list_agent_status, get_status_history, group CRUD, select_online_agent |
| `src/core_agent_api.rs` | Extended data structs: AgentRegisterRequest, AgentHeartbeatRequest, new AgentInfoRow, AgentStatusRow |
| `src/core/server.rs` | New HTTP endpoints, tcp_dispatch_loop updated |
| `src/core/tcp/listener.rs` | Heartbeat handling → update agent_status + insert history; registration → compute agent_id |
| `src/core/tcp/registry.rs` | No changes (heartbeat tracking already exists) |
| `src/agent/` | Resource detection module (sys_info), extended register/heartbeat messages |
| `pm-admin/src/types/api.ts` | New TypeScript interfaces |
| `pm-admin/src/api/agents.ts` | API functions for agents + groups |
| `pm-admin/src/api/hooks.ts` | React Query hooks |
| `pm-admin/src/pages/Agents/index.tsx` | Rewrite: 采集机信息 |
| `pm-admin/src/pages/Agents/StatusPage.tsx` | New: 实时状态 |
| `pm-admin/src/pages/Agents/HistoryPage.tsx` | New: 状态历史 (Recharts) |
| `pm-admin/src/pages/AgentGroups/index.tsx` | New: 采集机组 CRUD |
| `pm-admin/src/components/Layout.tsx` | Sub-menu under 采集机管理 |
| `pm-admin/src/App.tsx` | New routes |

---

### Task 1: DB Schema — Create 4 Tables, Drop Old `agents`

**Files:**
- Modify: `src/core/db.rs`

**Interfaces:**
- Produces: `init_schema()` creates 4 tables, drops old `agents`; unit test `test_agent_tables_exist`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn test_agent_tables_exist() {
    let db = CoreDb::open(":memory:").await.unwrap();
    // Verify agent_info exists
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_info")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(row.0, 0);
    // Verify agent_status exists
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_status")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(row.0, 0);
    // Verify agent_status_his exists
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_status_his")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(row.0, 0);
    // Verify agent_group exists
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_group")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(row.0, 0);
    // Verify old agents table is gone (expect error)
    let err = sqlx::query("SELECT COUNT(*) FROM agents")
        .fetch_one(&db.pool).await;
    assert!(err.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_agent_tables_exist 2>&1`
Expected: FAIL — tables don't exist yet, or old agents table still exists

- [ ] **Step 3: Replace `init_schema` table creation**

In `init_schema()`, find the `CREATE TABLE IF NOT EXISTS agents (...)` block and replace with:

```rust
// Drop old agents table
sqlx::query("DROP TABLE IF EXISTS agents")
    .execute(&self.pool).await?;

sqlx::query(
    r#"
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
    )
    "#,
)
.execute(&self.pool).await?;

sqlx::query(
    r#"
    CREATE TABLE IF NOT EXISTS agent_status (
        agent_id          INTEGER PRIMARY KEY,
        status            TEXT NOT NULL,
        cpu_load          REAL,
        memory_load       REAL,
        disk_load         REAL,
        heartbeat_time    TEXT NOT NULL,
        thread_num        INTEGER,
        description       TEXT
    )
    "#,
)
.execute(&self.pool).await?;

sqlx::query(
    r#"
    CREATE TABLE IF NOT EXISTS agent_status_his (
        agent_id          INTEGER NOT NULL,
        cpu_load          REAL,
        memory_load       REAL,
        disk_load         REAL,
        heartbeat_time    TEXT NOT NULL,
        thread_num        INTEGER,
        description       TEXT,
        insert_time       TEXT DEFAULT (datetime('now','localtime'))
    )
    "#,
)
.execute(&self.pool).await?;

sqlx::query(
    r#"
    CREATE INDEX IF NOT EXISTS idx_agent_status_his_agent_time
        ON agent_status_his(agent_id, heartbeat_time)
    "#,
)
.execute(&self.pool).await?;

sqlx::query(
    r#"
    CREATE TABLE IF NOT EXISTS agent_group (
        group_id    INTEGER PRIMARY KEY AUTOINCREMENT,
        group_name  TEXT NOT NULL,
        agent_ids   TEXT DEFAULT '[]' NOT NULL,
        description TEXT,
        time_stamp  TEXT
    )
    "#,
)
.execute(&self.pool).await?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_agent_tables_exist 2>&1`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/core/db.rs
git commit -m "feat: replace agents table with agent_info/status/his/group schema"
```

---

### Task 2: compute_agent_id + Extend AgentRegisterRequest + AgentHeartbeatRequest

**Files:**
- Create: `src/core/agent_id.rs`
- Modify: `src/core_agent_api.rs`

**Interfaces:**
- Produces: `compute_agent_id(host: &str, port: u16) -> i64` in `src/core/agent_id.rs`
- Produces: Extended `AgentRegisterRequest` (7 new Option fields)
- Produces: Extended `AgentHeartbeatRequest` (4 new Option fields)
- Consumed by: Task 3 (agent sends new fields), Task 4 (core computes agent_id)

- [ ] **Step 1: Create `src/core/agent_id.rs`**

```rust
use std::hash::{Hash, Hasher};

/// Compute a deterministic agent_id from host:port.
/// For valid IPv4 addresses: (ip_u32 * 65536 + port) as a positive i64.
/// For hostnames: negative hash to avoid collision with IP-based IDs.
pub fn compute_agent_id(host: &str, port: u16) -> i64 {
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        let ip_u32 = u32::from_be_bytes(ip.octets());
        (ip_u32 as i64) * 65536 + port as i64
    } else {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        host.hash(&mut hasher);
        port.hash(&mut hasher);
        let h = hasher.finish() as i64;
        if h == 0 { -1 } else { -h.abs() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_agent_id_ipv4() {
        let id = compute_agent_id("192.168.1.100", 9997);
        // 192.168.1.100 = 0xC0A80164 = 3232235876
        // 3232235876 * 65536 + 9997 = 211812126261293
        assert_eq!(id, 211812126261293);
    }

    #[test]
    fn test_compute_agent_id_hostname() {
        let id = compute_agent_id("agent-01.local", 9997);
        assert!(id < 0, "hostname should produce negative id");
    }

    #[test]
    fn test_compute_agent_id_uniqueness() {
        let id1 = compute_agent_id("10.0.0.1", 9997);
        let id2 = compute_agent_id("10.0.0.1", 9998);
        assert_ne!(id1, id2, "different ports should differ");
        let id3 = compute_agent_id("10.0.0.2", 9997);
        assert_ne!(id1, id3, "different IPs should differ");
    }
}
```

Add `pub mod agent_id;` to `src/core/mod.rs` (create if not exists, or add to existing).

- [ ] **Step 2: Run test to verify it fails** (module not compiled yet if mod not added — adjust steps)

Run: `cargo test test_compute_agent_id 2>&1`
Expected: PASS after adding module

- [ ] **Step 3: Extend `AgentRegisterRequest`**

In `src/core_agent_api.rs`, add fields to `AgentRegisterRequest`:

```rust
pub struct AgentRegisterRequest {
    pub agent_id: Option<String>,
    pub agent_name: String,
    pub host: String,
    pub port: u16,
    pub version: String,
    pub capabilities: AgentCapabilities,
    // New fields
    pub cpu_total: Option<String>,
    pub memory_total: Option<f64>,
    pub disk_total: Option<f64>,
    pub max_thread_num: Option<i32>,
    pub fact_memory_total: Option<f64>,
    pub heartbeat_interval: Option<i32>,
    pub is_core: Option<bool>,
}
```

- [ ] **Step 4: Extend `AgentHeartbeatRequest`**

```rust
pub struct AgentHeartbeatRequest {
    pub status: AgentStatus,
    pub running_task_ids: Vec<String>,
    pub disk_free_bytes: Option<u64>,
    // New fields
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub disk_load: Option<f64>,
    pub thread_num: Option<i32>,
}
```

- [ ] **Step 5: Update serde derives**

Ensure both structs have `#[derive(Clone, Debug, Deserialize, Serialize)]` — the new fields need these derives too. They already have them.

- [ ] **Step 6: Ensure old code compiles with new fields**

Run: `cargo test 2>&1 | tail -20`
Expected: All existing tests pass. Some code creating `AgentRegisterRequest` or `AgentHeartbeatRequest` may fail to compile due to missing new fields. Fix by adding default values:

```rust
AgentRegisterRequest {
    cpu_total: None,
    memory_total: None,
    disk_total: None,
    max_thread_num: None,
    fact_memory_total: None,
    heartbeat_interval: None,
    is_core: None,
    ..rest
}
```

(and similarly for HeartbeatRequest).

- [ ] **Step 7: Commit**

```bash
git add src/core/agent_id.rs src/core/mod.rs src/core_agent_api.rs
git commit -m "feat: add compute_agent_id, extend agent register/heartbeat messages"
```

---

### Task 3: Agent-Side Resource Detection & Extended Messages

**Files:**
- Modify: `src/agent/tcp.rs` (or wherever register/heartbeat messages are constructed)
- Modify: `src/bin/agent.rs` (if passing extra config)

**Interfaces:**
- Consumes: Extended `AgentRegisterRequest` and `AgentHeartbeatRequest` from Task 2
- Produces: Agent sends cpu_total/memory_total/disk_total on register, cpu_load/memory_load/disk_load on heartbeat

Find where the agent constructs its `AgentRegisterRequest` and `AgentHeartbeatRequest`:

- [ ] **Step 1: Find the register request construction**

Search for `AgentRegisterRequest {` in `src/agent/`:

```bash
rg "AgentRegisterRequest" src/agent/
```

Likely in `src/agent/tcp.rs`. Read the file to understand the current construction.

- [ ] **Step 2: Add system info detection on register**

In the register request construction, add:

```rust
// Detect system resources
let cpu_total = Some(format!("{} cores", num_cpus::get()));
let memory_total = sys_info::mem_info().map(|m| m.total as f64 / 1024.0).ok(); // MB
let disk_total = sys_info::disk_info().map(|d| d.total as f64 / (1024.0 * 1024.0 * 1024.0)).ok(); // GB
let max_thread_num = Some(num_cpus::get() as i32 * 2);
```

Add `sys-info` and `num_cpus` to `Cargo.toml` dependencies (check if already present).

If `sys-info` crate is not available, use `sysinfo` crate or implement a simple detection:
- CPU: `num_cpus::get()`
- Memory: read `/proc/meminfo` on Linux or use `sysctl` on macOS, or add `sysinfo` crate

- [ ] **Step 3: Add load detection on heartbeat**

In the heartbeat message construction (likely same file or `src/agent/server.rs`):

```rust
// Detect current load
let cpu_load = /* read /proc/stat or use sysinfo */;
let memory_load = /* current memory usage percentage */;
let disk_load = /* current disk usage percentage */;
let thread_num = /* current thread count */;
```

For simplicity, if system detection is complex, these can be set to `None` initially and implemented later. The core handles `None` gracefully.

- [ ] **Step 4: Test compilation**

Run: `cargo build 2>&1 | tail -10`
Expected: Build succeeds

- [ ] **Step 5: Commit**

```bash
git add src/agent/ Cargo.toml Cargo.lock
git commit -m "feat: agent reports system resources on register and load on heartbeat"
```

---

### Task 4: Core Registration Handler

**Files:**
- Modify: `src/core/db.rs` (new methods: upsert_agent_info, upsert_agent_status, remove_register_agent)
- Modify: `src/core/server.rs` (tcp_dispatch_loop registration handler)

**Interfaces:**
- Consumes: `compute_agent_id` from Task 2, extended `AgentRegisterRequest` from Task 2
- Produces: `upsert_agent_info()`, `upsert_agent_status()` in CoreDb

- [ ] **Step 1: Write tests for upsert_agent_info**

```rust
#[tokio::test]
async fn test_upsert_agent_info() {
    let db = CoreDb::open(":memory:").await.unwrap();
    let id = compute_agent_id("10.0.0.1", 9997);

    // Insert
    db.upsert_agent_info(id, "agent-01", "10.0.0.1", 9997, "1.0.0", None, None, None, None, None, None, false).await.unwrap();

    // Verify
    let row: (String,) = sqlx::query_as("SELECT agent_name FROM agent_info WHERE agent_id = ?")
        .bind(id).fetch_one(&db.pool).await.unwrap();
    assert_eq!(row.0, "agent-01");

    // Upsert (update name)
    db.upsert_agent_info(id, "agent-01-v2", "10.0.0.1", 9997, "1.0.0", None, None, None, None, None, None, false).await.unwrap();

    let row: (String,) = sqlx::query_as("SELECT agent_name FROM agent_info WHERE agent_id = ?")
        .bind(id).fetch_one(&db.pool).await.unwrap();
    assert_eq!(row.0, "agent-01-v2");
}
```

- [ ] **Step 2: Implement upsert_agent_info**

```rust
pub async fn upsert_agent_info(
    &self,
    agent_id: i64,
    agent_name: &str,
    agent_ip: &str,
    port: u16,
    version: &str,
    cpu_total: Option<&str>,
    memory_total: Option<f64>,
    disk_total: Option<f64>,
    max_thread_num: Option<i32>,
    fact_memory_total: Option<f64>,
    heartbeat_interval: Option<i32>,
    is_core: bool,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    sqlx::query(
        r#"
        INSERT INTO agent_info(agent_id, agent_name, agent_ip, port, version, cpu_total, memory_total, disk_total, max_thread_num, fact_memory_total, heartbeat_interval, is_core, registered_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(agent_id) DO UPDATE SET
            agent_name=excluded.agent_name,
            agent_ip=excluded.agent_ip,
            port=excluded.port,
            version=excluded.version,
            cpu_total=COALESCE(excluded.cpu_total, agent_info.cpu_total),
            memory_total=COALESCE(excluded.memory_total, agent_info.memory_total),
            disk_total=COALESCE(excluded.disk_total, agent_info.disk_total),
            max_thread_num=COALESCE(excluded.max_thread_num, agent_info.max_thread_num),
            fact_memory_total=COALESCE(excluded.fact_memory_total, agent_info.fact_memory_total),
            heartbeat_interval=COALESCE(excluded.heartbeat_interval, agent_info.heartbeat_interval),
            is_core=excluded.is_core,
            registered_at=excluded.registered_at
        "#,
    )
    .bind(agent_id)
    .bind(agent_name)
    .bind(agent_ip)
    .bind(port as i32)
    .bind(version)
    .bind(cpu_total)
    .bind(memory_total)
    .bind(disk_total)
    .bind(max_thread_num)
    .bind(fact_memory_total)
    .bind(heartbeat_interval)
    .bind(is_core as i32)
    .bind(&now)
    .execute(&self.pool)
    .await?;
    Ok(())
}
```

- [ ] **Step 3: Write tests for upsert_agent_status**

```rust
#[tokio::test]
async fn test_upsert_agent_status() {
    let db = CoreDb::open(":memory:").await.unwrap();
    let id = compute_agent_id("10.0.0.1", 9997);

    db.upsert_agent_status(id, "ONLINE").await.unwrap();

    let row: (String,) = sqlx::query_as("SELECT status FROM agent_status WHERE agent_id = ?")
        .bind(id).fetch_one(&db.pool).await.unwrap();
    assert_eq!(row.0, "ONLINE");

    // Upsert again (same status, no change needed, but should not error)
    db.upsert_agent_status(id, "ONLINE").await.unwrap();
}
```

- [ ] **Step 4: Implement upsert_agent_status**

```rust
pub async fn upsert_agent_status(&self, agent_id: i64, status: &str) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    sqlx::query(
        r#"
        INSERT INTO agent_status(agent_id, status, heartbeat_time)
        VALUES (?, ?, ?)
        ON CONFLICT(agent_id) DO UPDATE SET
            status=excluded.status,
            heartbeat_time=excluded.heartbeat_time
        "#,
    )
    .bind(agent_id)
    .bind(status)
    .bind(&now)
    .execute(&self.pool)
    .await?;
    Ok(())
}
```

- [ ] **Step 5: Update tcp_dispatch_loop in server.rs**

Replace the `AgentRegister` handler:

```rust
InternalMessage::AgentRegister(mut req) => {
    // Compute agent_id from host:port
    let agent_id = compute_agent_id(&req.host, req.port);
    req.agent_id = Some(agent_id.to_string());

    // Upsert agent_info
    if let Err(e) = db.upsert_agent_info(
        agent_id, &req.agent_name, &req.host, req.port, &req.version,
        req.cpu_total.as_deref(), req.memory_total, req.disk_total,
        req.max_thread_num, req.fact_memory_total, req.heartbeat_interval,
        req.is_core.unwrap_or(false),
    ).await {
        tracing::warn!(%agent_id, error = %e, "upsert agent_info failed");
    }

    // Upsert agent_status with ONLINE
    if let Err(e) = db.upsert_agent_status(agent_id, "ONLINE").await {
        tracing::warn!(%agent_id, error = %e, "upsert agent_status failed");
    }

    tracing::info!(%agent_id, "Agent registered in DB");
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test test_upsert_agent_info test_upsert_agent_status 2>&1`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/core/db.rs src/core/server.rs
git commit -m "feat: core registration handler upserts agent_info and agent_status"
```

---

### Task 5: Core Heartbeat Handler + Status History + TCP Timeout

**Files:**
- Modify: `src/core/db.rs` (update_agent_heartbeat, insert_status_his, mark_agent_offline)
- Modify: `src/core/tcp/listener.rs` (heartbeat handler)
- Modify: `src/core/server.rs` (tcp_cleanup_loop)

**Interfaces:**
- Consumes: `AgentHeartbeatRequest` with new load fields from Task 2
- Produces: `update_agent_heartbeat()`, `insert_status_his()`, `mark_agent_offline()`

- [ ] **Step 1: Write tests**

```rust
#[tokio::test]
async fn test_agent_heartbeat_flow() {
    let db = CoreDb::open(":memory:").await.unwrap();
    let id = compute_agent_id("10.0.0.1", 9997);
    db.upsert_agent_info(id, "a1", "10.0.0.1", 9997, "1.0", None, None, None, None, None, None, false).await.unwrap();
    db.upsert_agent_status(id, "ONLINE").await.unwrap();

    // Update heartbeat with load info
    db.update_agent_heartbeat(id, "ONLINE", Some(45.5), Some(60.0), Some(30.0), Some(8)).await.unwrap();

    let row: (String, Option<f64>, Option<f64>) = sqlx::query_as(
        "SELECT status, cpu_load, memory_load FROM agent_status WHERE agent_id = ?"
    ).bind(id).fetch_one(&db.pool).await.unwrap();
    assert_eq!(row.0, "ONLINE");
    assert!((row.1.unwrap() - 45.5).abs() < 0.01);
    assert!((row.2.unwrap() - 60.0).abs() < 0.01);

    // Verify history was written
    // (history insert is done separately — test insert_status_his)
}

#[tokio::test]
async fn test_insert_status_his() {
    let db = CoreDb::open(":memory:").await.unwrap();
    let id = compute_agent_id("10.0.0.1", 9997);
    db.upsert_agent_info(id, "a1", "10.0.0.1", 9997, "1.0", None, None, None, None, None, None, false).await.unwrap();

    db.insert_status_his(id, Some(50.0), Some(70.0), Some(20.0), Some(5)).await.unwrap();
    db.insert_status_his(id, Some(60.0), Some(65.0), Some(25.0), Some(6)).await.unwrap();

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_status_his WHERE agent_id = ?")
        .bind(id).fetch_one(&db.pool).await.unwrap();
    assert_eq!(count.0, 2);
}

#[tokio::test]
async fn test_mark_agent_offline() {
    let db = CoreDb::open(":memory:").await.unwrap();
    let id = compute_agent_id("10.0.0.1", 9997);
    db.upsert_agent_info(id, "a1", "10.0.0.1", 9997, "1.0", None, None, None, None, None, None, false).await.unwrap();
    db.upsert_agent_status(id, "ONLINE").await.unwrap();

    db.mark_agent_offline(id).await.unwrap();

    let status: String = sqlx::query_scalar("SELECT status FROM agent_status WHERE agent_id = ?")
        .bind(id).fetch_one(&db.pool).await.unwrap();
    assert_eq!(status, "OFFLINE");
}
```

- [ ] **Step 2: Implement methods in db.rs**

```rust
pub async fn update_agent_heartbeat(
    &self,
    agent_id: i64,
    status: &str,
    cpu_load: Option<f64>,
    memory_load: Option<f64>,
    disk_load: Option<f64>,
    thread_num: Option<i32>,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    sqlx::query(
        r#"
        UPDATE agent_status SET status=?, cpu_load=?, memory_load=?, disk_load=?, thread_num=?, heartbeat_time=?
        WHERE agent_id=?
        "#,
    )
    .bind(status)
    .bind(cpu_load)
    .bind(memory_load)
    .bind(disk_load)
    .bind(thread_num)
    .bind(&now)
    .bind(agent_id)
    .execute(&self.pool)
    .await?;
    Ok(())
}

pub async fn insert_status_his(
    &self,
    agent_id: i64,
    cpu_load: Option<f64>,
    memory_load: Option<f64>,
    disk_load: Option<f64>,
    thread_num: Option<i32>,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    sqlx::query(
        r#"
        INSERT INTO agent_status_his(agent_id, cpu_load, memory_load, disk_load, thread_num, heartbeat_time)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(agent_id)
    .bind(cpu_load)
    .bind(memory_load)
    .bind(disk_load)
    .bind(thread_num)
    .bind(&now)
    .execute(&self.pool)
    .await?;
    Ok(())
}

pub async fn mark_agent_offline(&self, agent_id: i64) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    sqlx::query(
        "UPDATE agent_status SET status='OFFLINE', heartbeat_time=? WHERE agent_id=?",
    )
    .bind(&now)
    .bind(agent_id)
    .execute(&self.pool)
    .await?;
    Ok(())
}
```

- [ ] **Step 3: Update listener.rs heartbeat handler**

Find where `InternalMessage::Heartbeat(_)` is handled in `src/core/tcp/listener.rs`. After updating the registry timestamp, add DB calls:

```rust
InternalMessage::Heartbeat(hb) => {
    registry.update_heartbeat(&agent_id).await;

    // Update agent_status and insert history
    if let Ok(agent_id_i64) = agent_id.parse::<i64>() {
        let _ = to_dispatch.send((agent_id.clone(), InternalMessage::Heartbeat(hb))).await;
    }

    let _ = registry.send(&agent_id, &InternalMessage::HeartbeatAck).await;
}
```

But wait — the heartbeat currently goes through the listener directly without going to `tcp_dispatch_loop`. The listener handles heartbeats inline. I need to forward the heartbeat data to the dispatch loop so it can update the DB.

However, looking at the current code, the listener calls `registry.update_heartbeat(&agent_id)` directly, and sends `HeartbeatAck` directly. It doesn't forward heartbeats to the dispatch loop.

The cleanest approach: send the heartbeat as an InternalMessage to the dispatch loop. But the heartbeat is already consumed in the match arm. Let me look at the exact code.

In `listener.rs` the current heartbeat handling is:
```rust
match &msg {
    InternalMessage::Heartbeat(_) => {
        registry.update_heartbeat(&agent_id).await;
        let _ = registry.send(&agent_id, &InternalMessage::HeartbeatAck).await;
    }
    _ => {
        if to_dispatch.send((agent_id.clone(), msg)).await.is_err() {
            break;
        }
    }
}
```

I need to also send the heartbeat data to the dispatch loop. I can change it to:

```rust
InternalMessage::Heartbeat(hb) => {
    registry.update_heartbeat(&agent_id).await;
    let _ = registry.send(&agent_id, &InternalMessage::HeartbeatAck).await;
    let _ = to_dispatch.send((agent_id.clone(), InternalMessage::Heartbeat(hb))).await;
}
```

Wait, this moves `hb` but we already consumed it in the match. Actually, the match is `match &msg` — it's a reference. So we can clone the heartbeat data:

Looking more carefully at the code, the match is:
```rust
match &msg {
    InternalMessage::Heartbeat(_) => {
        registry.update_heartbeat(&agent_id).await;
        let _ = registry.send(&agent_id, &InternalMessage::HeartbeatAck).await;
    }
    _ => {
        if to_dispatch.send((agent_id.clone(), msg)).await.is_err() {
            break;
        }
    }
}
```

`msg` is owned. The heartbeat arm doesn't move out of `msg` because it's using `_`. I can change the heartbeat arm to extract the data:

```rust
InternalMessage::Heartbeat(hb) => {
    registry.update_heartbeat(&agent_id).await;
    let _ = registry.send(&agent_id, &InternalMessage::HeartbeatAck).await;
    // Forward heartbeat data to dispatch loop for DB persistence
    let _ = to_dispatch.send((agent_id.clone(), InternalMessage::Heartbeat(hb.clone()))).await;
}
```

But we need `hb` to implement `Clone` — which it does (it derives Clone). Good.

Then in `tcp_dispatch_loop`, add a handler:

```rust
InternalMessage::Heartbeat(hb) => {
    let agent_id_i64 = agent_id.parse::<i64>().unwrap_or(0);
    if let Err(e) = db.update_agent_heartbeat(
        agent_id_i64, "ONLINE",
        hb.cpu_load, hb.memory_load, hb.disk_load, hb.thread_num,
    ).await {
        tracing::warn!(%agent_id, error = %e, "update heartbeat failed");
    }
    if let Err(e) = db.insert_status_his(
        agent_id_i64,
        hb.cpu_load, hb.memory_load, hb.disk_load, hb.thread_num,
    ).await {
        tracing::warn!(%agent_id, error = %e, "insert status_his failed");
    }
}
```

- [ ] **Step 4: Update tcp_cleanup_loop in server.rs**

Find the `tcp_cleanup_loop` that unregisters timed-out agents from the registry. After unregistering, add:

```rust
if let Ok(agent_id_i64) = agent_id.parse::<i64>() {
    if let Err(e) = db.mark_agent_offline(agent_id_i64).await {
        tracing::error!(%agent_id, error = %e, "mark agent offline failed");
    }
}
```

But wait — `tcp_cleanup_loop` currently doesn't have access to `db`. I need to pass it in.

Update the function signature:
```rust
async fn tcp_cleanup_loop(registry: ConnectionRegistry, db: CoreDb) {
```

And in `run_core_server`, pass `db.clone()` to `tcp_cleanup_loop`.

- [ ] **Step 5: Run tests**

Run: `cargo test test_agent_heartbeat_flow test_insert_status_his test_mark_agent_offline 2>&1`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/core/db.rs src/core/tcp/listener.rs src/core/server.rs
git commit -m "feat: core heartbeat handler updates agent_status, inserts history, marks offline on timeout"
```

---

### Task 6: select_online_agent New JOIN Query

**Files:**
- Modify: `src/core/db.rs` (replace `select_online_agent`)
- Modify: `src/core/server.rs` (update caller if return type changed)

**Interfaces:**
- Consumes: agent_info + agent_status tables
- Produces: `select_online_agent() -> Result<(i64, f64)>` (returns agent_id and agent_power)

- [ ] **Step 1: Write test**

```rust
#[tokio::test]
async fn test_select_online_agent() {
    let db = CoreDb::open(":memory:").await.unwrap();
    let id1 = compute_agent_id("10.0.0.1", 9997);
    let id2 = compute_agent_id("10.0.0.2", 9997);

    // Agent 1: ONLINE, isuse_flag=1
    db.upsert_agent_info(id1, "a1", "10.0.0.1", 9997, "1.0", None, None, None, None, None, None, false).await.unwrap();
    db.upsert_agent_status(id1, "ONLINE").await.unwrap();

    // Agent 2: ONLINE, isuse_flag=1
    db.upsert_agent_info(id2, "a2", "10.0.0.2", 9997, "1.0", None, None, None, None, None, None, false).await.unwrap();
    db.upsert_agent_status(id2, "ONLINE").await.unwrap();

    // Should return the most recent heartbeat
    let (aid, power) = db.select_online_agent().await.unwrap();
    assert!(aid == id1 || aid == id2);
    assert!((power - 1.0).abs() < 0.01);

    // Disable agent 1
    sqlx::query("UPDATE agent_info SET agent_isuse_flag=0 WHERE agent_id=?")
        .bind(id1).execute(&db.pool).await.unwrap();
    sqlx::query("UPDATE agent_status SET heartbeat_time='2020-01-01 00:00:00' WHERE agent_id=?")
        .bind(id2).execute(&db.pool).await.unwrap();

    let (aid, _) = db.select_online_agent().await.unwrap();
    assert_eq!(aid, id2); // Agent 2 is now the only enabled one

    // Disable agent 2
    sqlx::query("UPDATE agent_info SET agent_isuse_flag=0 WHERE agent_id=?")
        .bind(id2).execute(&db.pool).await.unwrap();

    let result = db.select_online_agent().await;
    assert!(result.is_err(), "no online+enabled agent should return error");
}
```

- [ ] **Step 2: Implement new select_online_agent**

Replace the old `select_online_agent` method:

```rust
pub async fn select_online_agent(&self) -> Result<(i64, f64)> {
    let row = sqlx::query_as::<_, (i64, f64)>(
        r#"
        SELECT ai.agent_id, COALESCE(ai.agent_power, 1.0)
        FROM agent_info ai
        JOIN agent_status ast ON ast.agent_id = ai.agent_id
        WHERE ast.status = 'ONLINE'
          AND ai.agent_isuse_flag = 1
        ORDER BY ast.heartbeat_time DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&self.pool)
    .await?;

    row.ok_or_else(|| anyhow::anyhow!("no online agent available"))
}
```

- [ ] **Step 3: Update dispatch_for_strategy in server.rs**

Find where `select_online_agent()` is called and update the destructuring to match new return type `(i64, f64)`:

```rust
let (agent_id, _agent_power) = state.db.select_online_agent().await?;
let agent_id_str = agent_id.to_string();
```

Then use `agent_id_str` instead of the old `agent_id` string for `create_task` and `registry.send`.

- [ ] **Step 4: Remove old list_all_agents and mark_stale_agents_offline**

These methods reference the old `agents` table. Either remove them or update them to use the new tables. The HTTP `GET /api/agents` handler currently calls `list_all_agents` — it will be replaced in Task 7.

For now, keep the old methods but expect some tests to fail until Task 7. Or update them immediately.

Simpler: Delete `list_all_agents` and old `register_agent` — tests that use them will be updated in Task 7.

- [ ] **Step 5: Run tests**

Run: `cargo test test_select_online_agent 2>&1`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/core/db.rs src/core/server.rs
git commit -m "feat: select_online_agent uses JOIN with agent_isuse_flag filter"
```

---

### Task 7: HTTP API — Agent Info, Agent Status, Agent Groups

**Files:**
- Modify: `src/core/db.rs` (new query methods for API)
- Modify: `src/core/server.rs` (new HTTP handlers + routes)
- Modify: `src/core_agent_api.rs` (new response types)

**Interfaces:**
- Consumes: DB methods from Tasks 4-6
- Produces: All HTTP API endpoints listed in spec

- [ ] **Step 1: Add query methods to db.rs**

```rust
// List all agents with their current status
pub async fn list_agents_with_status(&self) -> Result<Vec<AgentInfoRow>> {
    sqlx::query_as::<_, AgentInfoRow>(
        r#"
        SELECT ai.*, ast.status as current_status, ast.cpu_load, ast.memory_load, ast.disk_load,
               ast.thread_num as current_thread_num, ast.heartbeat_time as last_heartbeat_time
        FROM agent_info ai
        LEFT JOIN agent_status ast ON ast.agent_id = ai.agent_id
        ORDER BY ai.time_stamp DESC
        "#,
    )
    .fetch_all(&self.pool)
    .await
    .map_err(|e| anyhow::anyhow!("list agents: {e}"))
}

// Get single agent detail
pub async fn get_agent_detail(&self, agent_id: i64) -> Result<Option<AgentInfoRow>> {
    // similar query with WHERE ai.agent_id = ?
}

// Update agent (alias, isuse_flag, power, load_limit, description)
pub async fn update_agent_info(&self, agent_id: i64, alias: Option<&str>,
    isuse_flag: Option<i32>, power: Option<f64>, load_limit: Option<f64>,
    description: Option<&str>) -> Result<()>
{
    // Build dynamic UPDATE query (or simple per-field updates)
}

// List all agents' real-time status
pub async fn list_agent_status(&self) -> Result<Vec<AgentStatusRow>> {
    sqlx::query_as::<_, AgentStatusRow>(
        r#"
        SELECT ast.*, ai.agent_name
        FROM agent_status ast
        JOIN agent_info ai ON ai.agent_id = ast.agent_id
        ORDER BY ast.heartbeat_time DESC
        "#,
    )
    .fetch_all(&self.pool)
    .await
    .map_err(|e| anyhow::anyhow!("list status: {e}"))
}

// Get status history for one agent
pub async fn get_status_history(&self, agent_id: i64, limit: i32) -> Result<Vec<AgentStatusHisRow>> {
    sqlx::query_as::<_, AgentStatusHisRow>(
        r#"
        SELECT * FROM agent_status_his WHERE agent_id = ?
        ORDER BY heartbeat_time DESC LIMIT ?
        "#,
    )
    .bind(agent_id)
    .bind(limit)
    .fetch_all(&self.pool)
    .await
    .map_err(|e| anyhow::anyhow!("status history: {e}"))
}

// Group CRUD
pub async fn list_agent_groups(&self) -> Result<Vec<AgentGroupRow>> { ... }
pub async fn create_agent_group(&self, name: &str, agent_ids: &str, description: Option<&str>) -> Result<i64> { ... }
pub async fn update_agent_group(&self, group_id: i64, name: &str, agent_ids: &str, description: Option<&str>) -> Result<()> { ... }
pub async fn delete_agent_group(&self, group_id: i64) -> Result<()> { ... }
```

- [ ] **Step 2: Add response types to core_agent_api.rs**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentInfoRow {
    pub agent_id: i64,
    pub agent_name: String,
    pub agent_ip: String,
    pub port: i32,
    pub version: String,
    pub cpu_total: Option<String>,
    pub memory_total: Option<f64>,
    pub disk_total: Option<f64>,
    pub heartbeat_interval: Option<i32>,
    pub time_stamp: Option<String>,
    pub description: Option<String>,
    pub max_thread_num: Option<i32>,
    pub agent_isuse_flag: i32,
    pub fact_memory_total: Option<f64>,
    pub agent_alias: Option<String>,
    pub is_core: i32,
    pub agent_power: Option<f64>,
    pub host_load_limit: Option<f64>,
    pub registered_at: String,
    // Joined from agent_status
    pub current_status: Option<String>,
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub disk_load: Option<f64>,
    pub current_thread_num: Option<i32>,
    pub last_heartbeat_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentStatusRow {
    pub agent_id: i64,
    pub agent_name: String,
    pub status: String,
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub disk_load: Option<f64>,
    pub heartbeat_time: String,
    pub thread_num: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentStatusHisRow {
    pub agent_id: i64,
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub disk_load: Option<f64>,
    pub heartbeat_time: String,
    pub thread_num: Option<i32>,
    pub insert_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentGroupRow {
    pub group_id: i64,
    pub group_name: String,
    pub agent_ids: String,
    pub description: Option<String>,
    pub time_stamp: Option<String>,
}
```

- [ ] **Step 3: Add route registrations in server.rs**

```rust
.route("/api/agents/status", get(list_agent_status_handler))
.route("/api/agents/:id", get(get_agent_detail_handler).patch(update_agent_handler))
.route("/api/agents/:id/status-history", get(get_agent_status_history_handler))
.route("/api/agent-groups", get(list_agent_groups_handler).post(create_agent_group_handler))
.route("/api/agent-groups/:id", put(update_agent_group_handler).delete(delete_agent_group_handler))
```

Note: Order matters — `/api/agents/status` must be registered before `/api/agents/:id` to avoid "status" being matched as `:id`.

- [ ] **Step 4: Implement handlers**

```rust
async fn list_agents_handler(
    State(state): State<CoreState>,
) -> Response {
    match state.db.list_agents_with_status().await {
        Ok(agents) => ok_response(agents, "ok").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn get_agent_detail_handler(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
) -> Response { /* similar */ }

async fn update_agent_handler(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateAgentRequest>,
) -> Response { /* update fields */ }

async fn list_agent_status_handler(
    State(state): State<CoreState>,
) -> Response { /* list from agent_status JOIN agent_info */ }

async fn get_agent_status_history_handler(
    State(state): State<CoreState>,
    Path(id): Path<i64>,
    Query(params): Query<HistoryParams>,
) -> Response { /* limit defaults to 100 */ }

// Group handlers...
```

Also add `UpdateAgentRequest` struct and `HistoryParams`:

```rust
#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub agent_alias: Option<String>,
    pub agent_isuse_flag: Option<i32>,
    pub agent_power: Option<f64>,
    pub host_load_limit: Option<f64>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct HistoryParams {
    pub limit: Option<i32>,
}
```

- [ ] **Step 5: Update existing GET /api/agents handler**

Replace the existing `list_agents_handler` with the new one that calls `list_agents_with_status()`.

Remove the old `AgentInfo` struct usage from the handler (or keep it if `AgentInfoRow` is used instead).

- [ ] **Step 6: Run tests**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
git add src/core/db.rs src/core/server.rs src/core_agent_api.rs
git commit -m "feat: HTTP API for agent info, status, history, and groups"
```

---

### Task 8: Frontend — TypeScript Types + API Functions + Hooks

**Files:**
- Modify: `pm-admin/src/types/api.ts`
- Modify: `pm-admin/src/api/agents.ts`
- Modify: `pm-admin/src/api/hooks.ts`

- [ ] **Step 1: Add TypeScript types in `pm-admin/src/types/api.ts`**

```typescript
export interface AgentInfoRow {
  agent_id: number;
  agent_name: string;
  agent_ip: string;
  port: number;
  version: string;
  cpu_total: string | null;
  memory_total: number | null;
  disk_total: number | null;
  heartbeat_interval: number | null;
  time_stamp: string | null;
  description: string | null;
  max_thread_num: number | null;
  agent_isuse_flag: number;
  fact_memory_total: number | null;
  agent_alias: string | null;
  is_core: number;
  agent_power: number | null;
  host_load_limit: number | null;
  registered_at: string;
  // Joined from agent_status
  current_status: string | null;
  cpu_load: number | null;
  memory_load: number | null;
  disk_load: number | null;
  current_thread_num: number | null;
  last_heartbeat_time: string | null;
}

export interface AgentStatusRow {
  agent_id: number;
  agent_name: string;
  status: string;
  cpu_load: number | null;
  memory_load: number | null;
  disk_load: number | null;
  heartbeat_time: string;
  thread_num: number | null;
}

export interface AgentStatusHisRow {
  agent_id: number;
  cpu_load: number | null;
  memory_load: number | null;
  disk_load: number | null;
  heartbeat_time: string;
  thread_num: number | null;
  insert_time: string | null;
}

export interface AgentGroupRow {
  group_id: number;
  group_name: string;
  agent_ids: string;
  description: string | null;
  time_stamp: string | null;
}

export interface UpdateAgentRequest {
  agent_alias?: string;
  agent_isuse_flag?: number;
  agent_power?: number;
  host_load_limit?: number;
  description?: string;
}
```

- [ ] **Step 2: Update `pm-admin/src/api/agents.ts`**

```typescript
import http from './client';
import type { AgentInfoRow, AgentStatusRow, AgentStatusHisRow, AgentGroupRow, UpdateAgentRequest } from '../types/api';

// Agent Info
export function listAgents() {
  return http.get<AgentInfoRow[]>('/agents').then(r => r.data);
}

export function getAgentDetail(id: number) {
  return http.get<AgentInfoRow>(`/agents/${id}`).then(r => r.data);
}

export function updateAgent(id: number, data: UpdateAgentRequest) {
  return http.patch(`/agents/${id}`, data).then(r => r.data);
}

// Agent Status
export function listAgentStatus() {
  return http.get<AgentStatusRow[]>('/agents/status').then(r => r.data);
}

export function getAgentStatusHistory(id: number, limit?: number) {
  const params = limit ? `?limit=${limit}` : '';
  return http.get<AgentStatusHisRow[]>(`/agents/${id}/status-history${params}`).then(r => r.data);
}

// Agent Groups
export function listAgentGroups() {
  return http.get<AgentGroupRow[]>('/agent-groups').then(r => r.data);
}

export function createAgentGroup(data: { group_name: string; agent_ids?: string; description?: string }) {
  return http.post('/agent-groups', data).then(r => r.data);
}

export function updateAgentGroup(id: number, data: { group_name: string; agent_ids?: string; description?: string }) {
  return http.put(`/agent-groups/${id}`, data).then(r => r.data);
}

export function deleteAgentGroup(id: number) {
  return http.delete(`/agent-groups/${id}`).then(r => r.data);
}
```

- [ ] **Step 3: Update `pm-admin/src/api/hooks.ts`**

```typescript
export function useAgents() {
  return useQuery({
    queryKey: ['agents'],
    queryFn: listAgents,
    refetchInterval: 30_000,
  });
}

export function useAgentDetail(id: number) {
  return useQuery({
    queryKey: ['agent', id],
    queryFn: () => getAgentDetail(id),
    enabled: !!id,
  });
}

export function useAgentStatus() {
  return useQuery({
    queryKey: ['agent-status'],
    queryFn: listAgentStatus,
    refetchInterval: 10_000,
  });
}

export function useAgentStatusHistory(id: number, limit?: number) {
  return useQuery({
    queryKey: ['agent-status-history', id, limit],
    queryFn: () => getAgentStatusHistory(id, limit),
    enabled: !!id,
  });
}

export function useAgentGroups() {
  return useQuery({
    queryKey: ['agent-groups'],
    queryFn: listAgentGroups,
  });
}
```

- [ ] **Step 4: Remove old AgentInfo, AgentRegisterRequest types if no longer used**

Check if `AgentInfo` (old type) is used anywhere else. If not, remove it to avoid confusion.

- [ ] **Step 5: Commit**

```bash
git add pm-admin/src/types/api.ts pm-admin/src/api/agents.ts pm-admin/src/api/hooks.ts
git commit -m "feat(frontend): TypeScript types and API functions for agent management"
```

---

### Task 9: Frontend — 采集机信息 Page (rewrite)

**Files:**
- Rewrite: `pm-admin/src/pages/Agents/index.tsx`

- [ ] **Step 1: Rewrite AgentsPage.tsx**

Replace the entire file with a table showing all columns from spec:

```tsx
import { Table, Tag, Card, Typography, Space, Switch, Badge, Tooltip } from 'antd';
import { CloudServerOutlined, WifiOutlined, EditOutlined } from '@ant-design/icons';
import { useAgents } from '../../api/hooks';
import type { AgentInfoRow } from '../../types/api';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { updateAgent } from '../../api/agents';

const { Text } = Typography;

export default function AgentsPage() {
  const { data: agents, isLoading } = useAgents();
  const queryClient = useQueryClient();

  const toggleIsuse = useMutation({
    mutationFn: ({ id, flag }: { id: number; flag: number }) =>
      updateAgent(id, { agent_isuse_flag: flag }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['agents'] }),
  });

  const columns = [
    {
      title: 'ID',
      dataIndex: 'agent_id',
      key: 'agent_id',
      width: 100,
      render: (v: number) => <span className="mono">{v}</span>,
    },
    {
      title: '名称',
      dataIndex: 'agent_name',
      key: 'agent_name',
      render: (_: string, record: AgentInfoRow) => (
        <Space>
          <CloudServerOutlined style={{ fontSize: 18, color: '#64748B' }} />
          <div>
            <div style={{ fontWeight: 600 }}>{record.agent_name}</div>
            {record.agent_alias && (
              <Text type="secondary" style={{ fontSize: 12 }}>{record.agent_alias}</Text>
            )}
          </div>
        </Space>
      ),
    },
    {
      title: '名称',
      dataIndex: 'agent_alias',
      key: 'agent_alias',
    },
    {
      title: 'IP',
      dataIndex: 'agent_ip',
      key: 'agent_ip',
      render: (v: string) => <span className="mono">{v}</span>,
    },
    {
      title: '端口',
      dataIndex: 'port',
      key: 'port',
      render: (v: number) => <span className="mono">{v}</span>,
    },
    {
      title: '版本',
      dataIndex: 'version',
      key: 'version',
      render: (v: string) => <Tag className="mono">{v}</Tag>,
    },
    {
      title: 'CPU',
      dataIndex: 'cpu_total',
      key: 'cpu_total',
      render: (v: string | null) => v || '—',
    },
    {
      title: '内存(MB)',
      key: 'memory',
      render: (_: any, r: AgentInfoRow) => {
        if (r.fact_memory_total) return r.fact_memory_total.toFixed(0);
        if (r.memory_total) return r.memory_total.toFixed(0);
        return '—';
      },
    },
    {
      title: '磁盘(GB)',
      dataIndex: 'disk_total',
      key: 'disk_total',
      render: (v: number | null) => v ? v.toFixed(1) : '—',
    },
    {
      title: '类型',
      dataIndex: 'is_core',
      key: 'is_core',
      render: (v: number) => v === 1 ? <Tag color="red">核心机</Tag> : <Tag>采集机</Tag>,
    },
    {
      title: '权重',
      dataIndex: 'agent_power',
      key: 'agent_power',
      render: (v: number | null) => v ?? '—',
    },
    {
      title: '负载上限',
      dataIndex: 'host_load_limit',
      key: 'host_load_limit',
      render: (v: number | null) => v != null ? `${v}%` : '—',
    },
    {
      title: '心跳间隔',
      dataIndex: 'heartbeat_interval',
      key: 'heartbeat_interval',
      render: (v: number | null) => v ? `${v}s` : '—',
    },
    {
      title: '启用',
      dataIndex: 'agent_isuse_flag',
      key: 'agent_isuse_flag',
      render: (flag: number, record: AgentInfoRow) => (
        <Switch
          checked={flag === 1}
          onChange={(checked) =>
            toggleIsuse.mutate({ id: record.agent_id, flag: checked ? 1 : 0 })
          }
        />
      ),
    },
    {
      title: '描述',
      dataIndex: 'description',
      key: 'description',
      ellipsis: true,
      render: (v: string | null) => v || '—',
    },
    {
      title: '注册时间',
      dataIndex: 'registered_at',
      key: 'registered_at',
      render: (v: string) => <span className="mono">{v}</span>,
    },
    {
      title: '更新时间',
      dataIndex: 'time_stamp',
      key: 'time_stamp',
      render: (v: string | null) => v ? <span className="mono">{v}</span> : '—',
    },
  ];

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>采集机信息</h2>
        <p>已注册的采集机节点（启动时自动注册）</p>
      </div>
      <div className="page-body">
        <Card className="content-card" styles={{ body: { padding: 0 } }}>
          <Table<AgentInfoRow>
            className="data-table"
            rowKey="agent_id"
            dataSource={agents}
            columns={columns}
            loading={isLoading}
            pagination={false}
            scroll={{ x: 'max-content' }}
          />
        </Card>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Verify build**

Run: `cd pm-admin && npx tsc --noEmit 2>&1 | tail -20`
Expected: No type errors

- [ ] **Step 3: Commit**

```bash
git add pm-admin/src/pages/Agents/index.tsx
git commit -m "feat(frontend): rewrite 采集机信息 page with full fields"
```

---

### Task 10: Frontend — 实时状态 Page

**Files:**
- Create: `pm-admin/src/pages/Agents/StatusPage.tsx`

- [ ] **Step 1: Create StatusPage.tsx**

```tsx
import { Table, Tag, Card, Badge, Space, Typography } from 'antd';
import { CloudServerOutlined } from '@ant-design/icons';
import { useAgentStatus } from '../../api/hooks';
import type { AgentStatusRow } from '../../types/api';

const { Text } = Typography;

const statusColor: Record<string, string> = {
  ONLINE: '#22C55E',
  OFFLINE: '#EF4444',
};

export default function AgentStatusPage() {
  const { data: list, isLoading } = useAgentStatus();

  const columns = [
    {
      title: 'Agent ID',
      dataIndex: 'agent_id',
      key: 'agent_id',
      render: (v: number) => <span className="mono">{v}</span>,
    },
    {
      title: '名称',
      dataIndex: 'agent_name',
      key: 'agent_name',
      render: (v: string) => (
        <Space>
          <CloudServerOutlined />
          <span>{v}</span>
        </Space>
      ),
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      render: (s: string) => (
        <Space>
          <Badge color={statusColor[s] || '#94A3B8'} />
          {s === 'ONLINE' ? '在线' : s === 'OFFLINE' ? '离线' : s}
        </Space>
      ),
    },
    {
      title: 'CPU 负载',
      dataIndex: 'cpu_load',
      key: 'cpu_load',
      render: (v: number | null) => v != null ? `${v.toFixed(1)}%` : '—',
    },
    {
      title: '内存负载',
      dataIndex: 'memory_load',
      key: 'memory_load',
      render: (v: number | null) => v != null ? `${v.toFixed(1)}%` : '—',
    },
    {
      title: '磁盘负载',
      dataIndex: 'disk_load',
      key: 'disk_load',
      render: (v: number | null) => v != null ? `${v.toFixed(1)}%` : '—',
    },
    {
      title: '线程数',
      dataIndex: 'thread_num',
      key: 'thread_num',
      render: (v: number | null) => v ?? '—',
    },
    {
      title: '最后心跳',
      dataIndex: 'heartbeat_time',
      key: 'heartbeat_time',
      render: (v: string) => <span className="mono">{v}</span>,
    },
  ];

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>实时状态</h2>
        <p>采集机当前负载信息（每 10 秒自动刷新）</p>
      </div>
      <div className="page-body">
        <Card className="content-card" styles={{ body: { padding: 0 } }}>
          <Table<AgentStatusRow>
            className="data-table"
            rowKey="agent_id"
            dataSource={list}
            columns={columns}
            loading={isLoading}
            pagination={false}
          />
        </Card>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add pm-admin/src/pages/Agents/StatusPage.tsx
git commit -m "feat(frontend): add 实时状态 page with auto-refresh"
```

---

### Task 11: Frontend — 状态历史 Page (Recharts Line Chart)

**Files:**
- Create: `pm-admin/src/pages/Agents/HistoryPage.tsx`

- [ ] **Step 1: Create HistoryPage.tsx**

```tsx
import { useState } from 'react';
import { Card, Select, Space, Typography, Spin } from 'antd';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer } from 'recharts';
import { useAgents, useAgentStatusHistory } from '../../api/hooks';

const { Title } = Typography;

export default function AgentHistoryPage() {
  const { data: agents } = useAgents();
  const [selectedId, setSelectedId] = useState<number | undefined>(undefined);
  const { data: history, isLoading } = useAgentStatusHistory(selectedId!, 200);

  const options = (agents || []).map(a => ({
    value: a.agent_id,
    label: `${a.agent_name} (${a.agent_ip})`,
  }));

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>状态历史</h2>
        <p>采集机负载趋势分析</p>
      </div>
      <div className="page-body">
        <Card>
          <Space direction="vertical" style={{ width: '100%' }} size="large">
            <Select
              showSearch
              placeholder="选择采集机"
              options={options}
              value={selectedId}
              onChange={setSelectedId}
              style={{ width: 300 }}
              filterOption={(input, option) =>
                (option?.label as string)?.toLowerCase().includes(input.toLowerCase())
              }
            />
            {!selectedId && <Typography.Text type="secondary">请先选择一个采集机</Typography.Text>}
            {selectedId && isLoading && <Spin />}
            {selectedId && history && history.length > 0 && (
              <ResponsiveContainer width="100%" height={400}>
                <LineChart data={history}>
                  <CartesianGrid strokeDasharray="3 3" />
                  <XAxis
                    dataKey="heartbeat_time"
                    tick={{ fontSize: 11 }}
                    angle={-45}
                    textAnchor="end"
                    height={80}
                  />
                  <YAxis domain={[0, 100]} tickFormatter={v => `${v}%`} />
                  <Tooltip formatter={(v: number) => `${v.toFixed(1)}%`} />
                  <Legend />
                  <Line type="monotone" dataKey="cpu_load" stroke="#3B82F6" name="CPU 负载" dot={false} />
                  <Line type="monotone" dataKey="memory_load" stroke="#22C55E" name="内存负载" dot={false} />
                  <Line type="monotone" dataKey="disk_load" stroke="#F59E0B" name="磁盘负载" dot={false} />
                </LineChart>
              </ResponsiveContainer>
            )}
            {selectedId && history && history.length === 0 && (
              <Typography.Text type="secondary">暂无历史数据</Typography.Text>
            )}
          </Space>
        </Card>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add pm-admin/src/pages/Agents/HistoryPage.tsx
git commit -m "feat(frontend): add 状态历史 page with Recharts line chart"
```

---

### Task 12: Frontend — 采集机组 CRUD Page

**Files:**
- Create: `pm-admin/src/pages/AgentGroups/index.tsx`

- [ ] **Step 1: Create AgentGroupsPage.tsx**

```tsx
import { useState } from 'react';
import { Table, Card, Button, Modal, Form, Input, Space, Popconfirm, Typography, message } from 'antd';
import { PlusOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons';
import { useAgentGroups } from '../../api/hooks';
import { createAgentGroup, updateAgentGroup, deleteAgentGroup } from '../../api/agents';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import type { AgentGroupRow } from '../../types/api';

export default function AgentGroupsPage() {
  const { data: groups, isLoading } = useAgentGroups();
  const queryClient = useQueryClient();
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<AgentGroupRow | null>(null);
  const [form] = Form.useForm();

  const createMut = useMutation({
    mutationFn: (values: any) => createAgentGroup(values),
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['agent-groups'] }); message.success('创建成功'); },
  });

  const updateMut = useMutation({
    mutationFn: ({ id, values }: { id: number; values: any }) => updateAgentGroup(id, values),
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['agent-groups'] }); message.success('更新成功'); },
  });

  const deleteMut = useMutation({
    mutationFn: (id: number) => deleteAgentGroup(id),
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['agent-groups'] }); message.success('删除成功'); },
  });

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    setModalOpen(true);
  };

  const openEdit = (record: AgentGroupRow) => {
    setEditing(record);
    form.setFieldsValue(record);
    setModalOpen(true);
  };

  const handleOk = async () => {
    const values = await form.validateFields();
    if (editing) {
      updateMut.mutate({ id: editing.group_id, values });
    } else {
      createMut.mutate(values);
    }
    setModalOpen(false);
  };

  const columns = [
    { title: 'ID', dataIndex: 'group_id', key: 'group_id' },
    { title: '组名', dataIndex: 'group_name', key: 'group_name' },
    {
      title: 'Agent 数量',
      key: 'agent_count',
      render: (_: any, r: AgentGroupRow) => {
        try {
          return JSON.parse(r.agent_ids).length;
        } catch { return 0; }
      },
    },
    { title: '描述', dataIndex: 'description', key: 'description', ellipsis: true },
    { title: '时间', dataIndex: 'time_stamp', key: 'time_stamp' },
    {
      title: '操作',
      key: 'actions',
      render: (_: any, r: AgentGroupRow) => (
        <Space>
          <Button type="link" icon={<EditOutlined />} onClick={() => openEdit(r)}>编辑</Button>
          <Popconfirm title="确定删除?" onConfirm={() => deleteMut.mutate(r.group_id)}>
            <Button type="link" danger icon={<DeleteOutlined />}>删除</Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>采集机组</h2>
        <p>管理采集机组，用于分组调度</p>
      </div>
      <div className="page-body">
        <Card className="content-card" styles={{ body: { padding: 0 } }}>
          <div style={{ padding: 16, textAlign: 'right' }}>
            <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>新建组</Button>
          </div>
          <Table<AgentGroupRow>
            className="data-table"
            rowKey="group_id"
            dataSource={groups}
            columns={columns}
            loading={isLoading}
            pagination={false}
          />
        </Card>
      </div>

      <Modal
        title={editing ? '编辑组' : '新建组'}
        open={modalOpen}
        onOk={handleOk}
        onCancel={() => setModalOpen(false)}
      >
        <Form form={form} layout="vertical">
          <Form.Item name="group_name" label="组名" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label="描述">
            <Input.TextArea rows={3} />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add pm-admin/src/pages/AgentGroups/index.tsx
git commit -m "feat(frontend): add 采集机组 CRUD page"
```

---

### Task 13: Frontend — Sub-Menu + Router + Layout

**Files:**
- Modify: `pm-admin/src/App.tsx`
- Modify: `pm-admin/src/components/Layout.tsx`

- [ ] **Step 1: Add routes in App.tsx**

```typescript
import AgentStatusPage from './pages/Agents/StatusPage';
import AgentHistoryPage from './pages/Agents/HistoryPage';
import AgentGroupsPage from './pages/AgentGroups';

// Add routes under the Layout route
<Route path="/agents" element={<AgentsPage />} />
<Route path="/agents/status" element={<AgentStatusPage />} />
<Route path="/agents/history" element={<AgentHistoryPage />} />
<Route path="/agent-groups" element={<AgentGroupsPage />} />
```

- [ ] **Step 2: Add sub-menu in Layout.tsx**

Find where the sidebar/collapsed menu items are defined. Currently likely:

```typescript
{
  key: '/agents',
  icon: <CloudServerOutlined />,
  label: '采集机管理',
}
```

Change to a sub-menu:

```typescript
{
  key: 'agents-group',
  icon: <CloudServerOutlined />,
  label: '采集机管理',
  children: [
    { key: '/agents', label: '采集机信息' },
    { key: '/agents/status', label: '实时状态' },
    { key: '/agents/history', label: '状态历史' },
    { key: '/agent-groups', label: '采集机组' },
  ],
}
```

- [ ] **Step 3: Verify build**

Run: `cd pm-admin && npx tsc --noEmit 2>&1`
Expected: No type errors

- [ ] **Step 4: Commit**

```bash
git add pm-admin/src/App.tsx pm-admin/src/components/Layout.tsx
git commit -m "feat(frontend): add sub-menu and routes for agent management pages"
```

---

### Task 14: Fix Existing Tests + Final Integration

**Files:** Various test files

- [ ] **Step 1: Run full test suite**

```bash
cargo test 2>&1 | grep -E "(FAIL|test result|error)"
```

Expected: Some tests may fail due to removed `agents` table, removed `list_all_agents`, or changed `select_online_agent` return type.

- [ ] **Step 2: Fix failing tests**

Common failures:
- Tests using old `agents` table → update to use new tables
- Tests calling `list_all_agents` → replace with `list_agents_with_status`
- Tests calling `register_agent` → replace with `upsert_agent_info` + `upsert_agent_status`
- Tests calling `mark_stale_agents_offline` → remove or update to use new tables

- [ ] **Step 3: Remove dead code**

Remove `list_all_agents`, `register_agent`, `mark_stale_agents_offline`, and old `AgentInfo` struct if no longer used.

- [ ] **Step 4: Verify all tests pass**

```bash
cargo test 2>&1 | tail -5
```
Expected: `test result: ok. X passed; 0 failed`

- [ ] **Step 5: Build release + copy to test/**

```bash
cargo build --release && cp target/release/core test/core && cp target/release/agent test/agent && cp server.toml agent.toml test/
```

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "fix: update tests and remove dead code for agent management"
```
