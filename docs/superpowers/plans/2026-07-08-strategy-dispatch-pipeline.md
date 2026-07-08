# Strategy Dispatch Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a unified Core-side strategy dispatch pipeline with `collect_tasks.group_id` self-association, load-balanced Agent selection, retry handling, and heartbeat failure recovery.

**Architecture:** Keep Agent TCP execution compatible by continuing to send existing `DispatchTask` messages, while Core internally treats related tasks as a task group through `collect_tasks.group_id`. Add focused Core dispatch helpers for group ID generation, validation, candidate expansion, load balancing, retry bookkeeping, and status updates before replacing direct dispatch paths.

**Tech Stack:** Rust, Tokio, Axum, sqlx, SQLite, bincode TCP messages, existing `crc64_ecma` ID utility.

## Global Constraints

- Do not create a new task group table.
- Use `collect_tasks.group_id` as the task group self-association key.
- Generate `group_id` independently; do not reuse the first `task_id`.
- First implementation keeps TCP dispatch as multiple existing `DispatchTask` messages.
- Keep all CRC64/i64 JSON IDs as strings at API boundaries; do not convert frontend IDs to `number`.
- All static SQL added to `src/core/db.rs` must use `trace_sql!` before execution.
- Do not commit during implementation unless the user explicitly requests commits.
- Verification command for Rust changes is `cargo test`.

---

## File Structure

| File | Responsibility |
|------|----------------|
| `src/core_agent_api.rs` | Add optional `group_id` to `TaskDispatchRequest` for traceability. |
| `src/core/db.rs` | Add `collect_tasks` columns, group-aware task creation/status/retry methods, candidate Agent queries, and tests. |
| `src/core/server.rs` | Add Core dispatch types and loops, replace direct strategy dispatch path, integrate load balancing and heartbeat failure handling. |
| `src/message.rs` | No first-stage message enum change; keep existing `DispatchTask`. |
| `src/agent/server.rs` | Send `DispatchTaskAck` after accepting a task. |
| `src/agent/tcp.rs` | Track running task IDs for heartbeat in a later task only if needed for capacity accuracy. |
| `docs/superpowers/specs/2026-07-08-strategy-dispatch-pipeline-design.md` | Design source of truth. |

---

### Task 1: Extend Task Schema And Dispatch Request

**Files:**
- Modify: `src/core/db.rs`
- Modify: `src/core_agent_api.rs`

**Interfaces:**
- Consumes: existing `CoreDb::create_task(...)` callers in `src/core/server.rs`.
- Produces: `TaskDispatchRequest { group_id: Option<String>, ... }` and `CoreDb::create_task(..., group_id: &str)`.

- [ ] **Step 1: Write failing tests for group columns and task creation**

Add this test inside `#[cfg(test)] mod tests` in `src/core/db.rs`:

```rust
    #[tokio::test]
    async fn create_task_persists_group_metadata() {
        let db = test_db().await;
        db.create_task(
            "task_grouped_1",
            "strategy_1:2026-07-08 10:00:00",
            "1",
            "snapshot_1",
            "2026-07-08 10:00:00",
            "collect_1",
            "agent_1",
            "group_123",
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT group_id, retry_count, next_retry_at, dispatch_error FROM collect_tasks WHERE task_id = ?",
        )
        .bind("task_grouped_1")
        .fetch_one(&db.pool)
        .await
        .unwrap();

        assert_eq!(row.get::<String, _>("group_id"), "group_123");
        assert_eq!(row.get::<i64, _>("retry_count"), 0);
        assert!(row.get::<Option<String>, _>("next_retry_at").is_none());
        assert!(row.get::<Option<String>, _>("dispatch_error").is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test create_task_persists_group_metadata`

Expected: FAIL because `create_task` has no `group_id` argument or the columns do not exist.

- [ ] **Step 3: Add schema columns in `init_schema()`**

In `src/core/db.rs`, after the `collect_tasks` table creation block, add idempotent migrations:

```rust
        let _ = sqlx::query("ALTER TABLE collect_tasks ADD COLUMN group_id TEXT")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("ALTER TABLE collect_tasks ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("ALTER TABLE collect_tasks ADD COLUMN next_retry_at TEXT")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("ALTER TABLE collect_tasks ADD COLUMN dispatch_error TEXT")
            .execute(&self.pool)
            .await;
```

Then update the `CREATE TABLE collect_tasks` SQL to include the same columns for fresh databases:

```sql
                group_id TEXT,
                retry_count INTEGER NOT NULL DEFAULT 0,
                next_retry_at TEXT,
                dispatch_error TEXT,
```

- [ ] **Step 4: Update `create_task` signature and SQL**

Change `CoreDb::create_task` signature to:

```rust
    pub async fn create_task(
        &self,
        task_id: &str,
        logical_task_key: &str,
        strategy_id: &str,
        config_snapshot_id: &str,
        scan_start_time: &str,
        collect_id: &str,
        assigned_agent_id: &str,
        group_id: &str,
    ) -> Result<()> {
```

Replace the insert SQL with:

```rust
        trace_sql!("INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, group_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'CREATED', ?)", task_id = task_id, logical_task_key = logical_task_key, strategy_id = strategy_id, config_snapshot_id = config_snapshot_id, scan_start_time = scan_start_time, collect_id = collect_id, assigned_agent_id = assigned_agent_id, group_id = group_id);
        sqlx::query(
            "INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, group_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'CREATED', ?)",
        )
        .bind(task_id)
        .bind(logical_task_key)
        .bind(strategy_id)
        .bind(config_snapshot_id)
        .bind(scan_start_time)
        .bind(collect_id)
        .bind(assigned_agent_id)
        .bind(group_id)
        .bind(&now)
        .execute(&self.pool)
        .await?;
```

- [ ] **Step 5: Add `group_id` to `TaskDispatchRequest`**

In `src/core_agent_api.rs`, add after `strategy_id`:

```rust
    pub group_id: Option<String>,
```

Update the `TaskDispatchRequest` struct construction sites in `src/core/server.rs` to include:

```rust
        group_id: Some(group_id.clone()),
```

For existing tests or literal constructors, use:

```rust
        group_id: None,
```

- [ ] **Step 6: Update current `create_task` call sites**

In `src/core/server.rs`, update both current calls to pass a group ID. Before Task 3 introduces stable group generation, use the task ID as temporary compatibility value at the call site:

```rust
            &request.task_id,
```

as the final argument.

- [ ] **Step 7: Run focused test**

Run: `cargo test create_task_persists_group_metadata`

Expected: PASS.

- [ ] **Step 8: Run broader compile check**

Run: `cargo test task_status_serializes_as_screaming_snake_case`

Expected: PASS and no compile errors from changed struct fields.

- [ ] **Step 9: Commit checkpoint**

Skip this step unless the user explicitly asks to commit. If requested, run:

```bash
git add src/core/db.rs src/core_agent_api.rs src/core/server.rs
git commit -m "feat: add task group metadata to collect tasks"
```

---

### Task 2: Add Group-Aware DB Operations

**Files:**
- Modify: `src/core/db.rs`

**Interfaces:**
- Consumes: `collect_tasks.group_id`, `retry_count`, `next_retry_at`, `dispatch_error` from Task 1.
- Produces:
  - `CoreDb::assign_group_to_agent(group_id: &str, agent_id: &str) -> Result<u64>`
  - `CoreDb::update_group_status(group_id: &str, status: &str, error_message: Option<&str>) -> Result<u64>`
  - `CoreDb::increment_group_retry(group_id: &str, next_retry_at: &str, error_message: &str) -> Result<u64>`
  - `CoreDb::count_active_tasks_by_agent(agent_id: &str) -> Result<i64>`
  - `CoreDb::mark_active_tasks_failed_for_agent(agent_id: &str, reason: &str) -> Result<u64>`

- [ ] **Step 1: Write failing DB tests**

Add these tests in `src/core/db.rs` test module:

```rust
    #[tokio::test]
    async fn group_status_and_retry_updates_all_non_terminal_tasks() {
        let db = test_db().await;
        for task_id in ["task_g1_a", "task_g1_b"] {
            db.create_task(
                task_id,
                task_id,
                "1",
                "snapshot_1",
                "2026-07-08 10:00:00",
                task_id,
                "agent_old",
                "group_g1",
            )
            .await
            .unwrap();
        }

        let assigned = db.assign_group_to_agent("group_g1", "agent_new").await.unwrap();
        assert_eq!(assigned, 2);

        let updated = db.update_group_status("group_g1", "DISPATCHING", None).await.unwrap();
        assert_eq!(updated, 2);

        let retried = db
            .increment_group_retry("group_g1", "2026-07-08 10:01:00", "no available agent")
            .await
            .unwrap();
        assert_eq!(retried, 2);

        let row = sqlx::query(
            "SELECT assigned_agent_id, status, retry_count, next_retry_at, dispatch_error FROM collect_tasks WHERE task_id = ?",
        )
        .bind("task_g1_a")
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("assigned_agent_id"), "agent_new");
        assert_eq!(row.get::<String, _>("status"), "DISPATCHING");
        assert_eq!(row.get::<i64, _>("retry_count"), 1);
        assert_eq!(row.get::<String, _>("next_retry_at"), "2026-07-08 10:01:00");
        assert_eq!(row.get::<String, _>("dispatch_error"), "no available agent");
    }

    #[tokio::test]
    async fn active_task_count_and_agent_failure_ignore_terminal_tasks() {
        let db = test_db().await;
        for (task_id, status) in [("task_active_1", "CREATED"), ("task_active_2", "RUNNING"), ("task_done", "SUCCEEDED")] {
            db.create_task(
                task_id,
                task_id,
                "1",
                "snapshot_1",
                "2026-07-08 10:00:00",
                task_id,
                "agent_1",
                "group_active",
            )
            .await
            .unwrap();
            sqlx::query("UPDATE collect_tasks SET status = ? WHERE task_id = ?")
                .bind(status)
                .bind(task_id)
                .execute(&db.pool)
                .await
                .unwrap();
        }

        assert_eq!(db.count_active_tasks_by_agent("agent_1").await.unwrap(), 2);

        let failed = db
            .mark_active_tasks_failed_for_agent("agent_1", "agent heartbeat timeout")
            .await
            .unwrap();
        assert_eq!(failed, 2);
        assert_eq!(db.count_active_tasks_by_agent("agent_1").await.unwrap(), 0);
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test group_`

Expected: FAIL because the new methods are undefined.

- [ ] **Step 3: Add terminal status helper**

Near the `CoreDb` impl, add a SQL predicate constant:

```rust
const NON_TERMINAL_TASK_STATUS_SQL: &str = "status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')";
```

- [ ] **Step 4: Implement group assignment and status methods**

Add methods inside `impl CoreDb`:

```rust
    pub async fn assign_group_to_agent(&self, group_id: &str, agent_id: &str) -> Result<u64> {
        trace_sql!("UPDATE collect_tasks SET assigned_agent_id = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')", agent_id = agent_id, group_id = group_id);
        let result = sqlx::query(
            "UPDATE collect_tasks SET assigned_agent_id = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')",
        )
        .bind(agent_id)
        .bind(group_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn update_group_status(&self, group_id: &str, status: &str, error_message: Option<&str>) -> Result<u64> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("UPDATE collect_tasks SET status = ?, dispatch_error = ?, last_progress_at = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')", status = status, error_message = error_message, group_id = group_id);
        let result = sqlx::query(
            "UPDATE collect_tasks SET status = ?, dispatch_error = ?, last_progress_at = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')",
        )
        .bind(status)
        .bind(error_message)
        .bind(&now)
        .bind(group_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn increment_group_retry(&self, group_id: &str, next_retry_at: &str, error_message: &str) -> Result<u64> {
        trace_sql!("UPDATE collect_tasks SET retry_count = retry_count + 1, next_retry_at = ?, dispatch_error = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')", next_retry_at = next_retry_at, error_message = error_message, group_id = group_id);
        let result = sqlx::query(
            "UPDATE collect_tasks SET retry_count = retry_count + 1, next_retry_at = ?, dispatch_error = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')",
        )
        .bind(next_retry_at)
        .bind(error_message)
        .bind(group_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
```

- [ ] **Step 5: Implement Agent task count and failure methods**

Add methods inside `impl CoreDb`:

```rust
    pub async fn count_active_tasks_by_agent(&self, agent_id: &str) -> Result<i64> {
        trace_sql!("SELECT COUNT(*) FROM collect_tasks WHERE assigned_agent_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')", agent_id = agent_id);
        let count = sqlx::query_scalar(
            "SELECT COUNT(*) FROM collect_tasks WHERE assigned_agent_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')",
        )
        .bind(agent_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    pub async fn mark_active_tasks_failed_for_agent(&self, agent_id: &str, reason: &str) -> Result<u64> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("UPDATE collect_tasks SET status = 'FAILED', finished_at = ?, dispatch_error = ? WHERE assigned_agent_id = ? AND status IN ('CREATED', 'DISPATCHING', 'ACCEPTED', 'RUNNING')", agent_id = agent_id, reason = reason);
        let result = sqlx::query(
            "UPDATE collect_tasks SET status = 'FAILED', finished_at = ?, dispatch_error = ? WHERE assigned_agent_id = ? AND status IN ('CREATED', 'DISPATCHING', 'ACCEPTED', 'RUNNING')",
        )
        .bind(&now)
        .bind(reason)
        .bind(agent_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
```

- [ ] **Step 6: Run focused tests**

Run: `cargo test group_`

Expected: PASS.

- [ ] **Step 7: Remove unused helper if compiler reports it**

If `NON_TERMINAL_TASK_STATUS_SQL` is unused and causes a warning only, either use it in methods if converting SQL to dynamic strings, or remove it. Do not leave unused code if the compiler fails.

- [ ] **Step 8: Commit checkpoint**

Skip this step unless the user explicitly asks to commit. If requested, run:

```bash
git add src/core/db.rs
git commit -m "feat: add group task db operations"
```

---

### Task 3: Add Dispatch Domain Types And Deterministic Group ID

**Files:**
- Modify: `src/core/server.rs`

**Interfaces:**
- Consumes: `crate::crc64::crc64_ecma`.
- Produces:
  - `StrategyCommand`
  - `StrategyCommandSource`
  - `TaskGroup`
  - `fn compute_task_group_id(strategy_id: &str, collector_id: i64, scan_start_time: &str, scan_end_time: Option<&str>, table_names: &[String]) -> String`

- [ ] **Step 1: Write failing tests for group ID stability**

Add tests inside `#[cfg(test)] mod tests` in `src/core/server.rs`:

```rust
    #[test]
    fn task_group_id_is_stable_when_table_order_changes() {
        let a = compute_task_group_id(
            "101",
            202,
            "2026-07-08 10:00:00",
            Some("2026-07-08 10:15:00"),
            &["TPD_B".to_string(), "TPD_A".to_string()],
        );
        let b = compute_task_group_id(
            "101",
            202,
            "2026-07-08 10:00:00",
            Some("2026-07-08 10:15:00"),
            &["TPD_A".to_string(), "TPD_B".to_string()],
        );

        assert_eq!(a, b);
        assert!(a.starts_with("group_"));
    }

    #[test]
    fn task_group_id_changes_for_different_window() {
        let a = compute_task_group_id("101", 202, "2026-07-08 10:00:00", None, &["TPD_A".to_string()]);
        let b = compute_task_group_id("101", 202, "2026-07-08 10:15:00", None, &["TPD_A".to_string()]);
        assert_ne!(a, b);
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test task_group_id_`

Expected: FAIL because `compute_task_group_id` is undefined.

- [ ] **Step 3: Add dispatch domain types**

In `src/core/server.rs`, near `CoreState`, add:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
enum StrategyCommandSource {
    Immediate,
    Periodic,
    Backfill,
}

#[derive(Clone, Debug)]
struct StrategyCommand {
    source: StrategyCommandSource,
    strategy: CollectionStrategyRow,
    unit: DataCollectorUnitRow,
    config_snapshot_id: String,
    scan_start_time: String,
    scan_end_time: Option<String>,
    table_names: Vec<String>,
    force_agent_id: Option<String>,
}

#[derive(Clone, Debug)]
struct TaskGroup {
    group_id: String,
    source: StrategyCommandSource,
    strategy_ids: Vec<String>,
    collector_id: i64,
    collector_name: String,
    candidate_ids: Vec<String>,
    scan_start_time: String,
    scan_end_time: Option<String>,
    table_names: Vec<String>,
    config_snapshot_id: String,
    force_agent_id: Option<String>,
    retry_count: u32,
}
```

- [ ] **Step 4: Implement group ID function**

Add below the structs:

```rust
fn compute_task_group_id(
    strategy_id: &str,
    collector_id: i64,
    scan_start_time: &str,
    scan_end_time: Option<&str>,
    table_names: &[String],
) -> String {
    let mut sorted_tables = table_names.to_vec();
    sorted_tables.sort();
    let input = format!(
        "{}|{}|{}|{}|{}",
        strategy_id,
        collector_id,
        scan_start_time,
        scan_end_time.unwrap_or(""),
        sorted_tables.join(",")
    );
    format!("group_{}", crate::crc64::crc64_ecma(&input))
}
```

- [ ] **Step 5: Run focused tests**

Run: `cargo test task_group_id_`

Expected: PASS.

- [ ] **Step 6: Commit checkpoint**

Skip this step unless the user explicitly asks to commit. If requested, run:

```bash
git add src/core/server.rs
git commit -m "feat: add strategy dispatch domain types"
```

---

### Task 4: Add Candidate Expansion And Load Balancing Queries

**Files:**
- Modify: `src/core_agent_api.rs`
- Modify: `src/core/db.rs`
- Modify: `src/core/server.rs`

**Interfaces:**
- Consumes: Agent status data in `agent_info`, `agent_status`, and `agent_group`.
- Produces:
  - `AgentDispatchCandidate`
  - `CoreDb::list_dispatch_candidates(agent_ids: &[String]) -> Result<Vec<AgentDispatchCandidate>>`
  - `CoreDb::expand_agent_group(group_id: i64) -> Result<Vec<String>>`
  - `async fn select_agent_for_group(...) -> Result<Option<String>>`

- [ ] **Step 1: Add candidate row type**

In `src/core_agent_api.rs`, add near `AgentStatusRow`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentDispatchCandidate {
    #[serde(with = "serde_i64")]
    pub agent_id: i64,
    pub agent_name: String,
    pub agent_alias: Option<String>,
    pub agent_isuse_flag: i32,
    pub agent_power: Option<f64>,
    pub host_load_limit: Option<f64>,
    pub current_status: Option<String>,
    pub cpu_load: Option<f64>,
    pub memory_load: Option<f64>,
    pub current_thread_num: Option<i32>,
    pub last_heartbeat_time: Option<String>,
}
```

Add it to the `use crate::core_agent_api::{ ... }` list in `src/core/db.rs`.

- [ ] **Step 2: Write failing DB test for group expansion**

Add in `src/core/db.rs` tests:

```rust
    #[tokio::test]
    async fn expand_agent_group_returns_member_ids() {
        let db = test_db().await;
        let group_id = crate::crc64::crc64_ecma("dispatch-group");
        sqlx::query("INSERT INTO agent_group(group_id, group_name, agent_ids, time_stamp) VALUES (?, ?, ?, ?)")
            .bind(group_id)
            .bind("dispatch-group")
            .bind("[\"11\",\"22\"]")
            .bind("2026-07-08 10:00:00")
            .execute(&db.pool)
            .await
            .unwrap();

        let ids = db.expand_agent_group(group_id).await.unwrap();
        assert_eq!(ids, vec!["11".to_string(), "22".to_string()]);
    }
```

- [ ] **Step 3: Implement `expand_agent_group`**

In `src/core/db.rs`:

```rust
    pub async fn expand_agent_group(&self, group_id: i64) -> Result<Vec<String>> {
        trace_sql!("SELECT agent_ids FROM agent_group WHERE group_id = ?", group_id = group_id);
        let agent_ids: Option<String> = sqlx::query_scalar("SELECT agent_ids FROM agent_group WHERE group_id = ?")
            .bind(group_id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(agent_ids) = agent_ids else { return Ok(Vec::new()); };
        let ids = serde_json::from_str::<Vec<String>>(&agent_ids)
            .or_else(|_| Ok::<Vec<String>, serde_json::Error>(agent_ids.split(',').map(str::trim).filter(|s| !s.is_empty()).map(ToOwned::to_owned).collect()))?;
        Ok(ids)
    }
```

- [ ] **Step 4: Implement `list_dispatch_candidates`**

In `src/core/db.rs`:

```rust
    pub async fn list_dispatch_candidates(&self, agent_ids: &[String]) -> Result<Vec<AgentDispatchCandidate>> {
        if agent_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat("?").take(agent_ids.len()).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT ai.agent_id, ai.agent_name, ai.agent_alias, ai.agent_isuse_flag, ai.agent_power, ai.host_load_limit, ast.status as current_status, ast.cpu_load, ast.memory_load, ast.thread_num as current_thread_num, ast.heartbeat_time as last_heartbeat_time FROM agent_info ai LEFT JOIN agent_status ast ON ast.agent_id = ai.agent_id WHERE ai.agent_id IN ({})",
            placeholders
        );
        tracing::info!("[db] ==> {}  Parameters: {:?}", sql, agent_ids);
        let mut query = sqlx::query_as::<_, AgentDispatchCandidate>(&sql);
        for agent_id in agent_ids {
            query = query.bind(agent_id.parse::<i64>().unwrap_or(0));
        }
        query.fetch_all(&self.pool).await.map_err(Into::into)
    }
```

- [ ] **Step 5: Write failing server test for scoring**

Add a pure helper in Task 4 before async DB selection if needed. Test this in `src/core/server.rs`:

```rust
    #[test]
    fn score_agent_prefers_more_available_capacity() {
        let busy = score_agent(2.0, 2, 1, 1.0);
        let idle = score_agent(4.0, 1, 1, 1.0);
        assert!(idle > busy);
    }
```

- [ ] **Step 6: Implement scoring helper**

In `src/core/server.rs`:

```rust
fn score_agent(agent_power: f64, running_task_count: i64, new_task_count: i64, factor: f64) -> f64 {
    let total_task_count = (running_task_count + new_task_count).max(1) as f64;
    (agent_power / total_task_count) * factor - running_task_count as f64
}
```

- [ ] **Step 7: Implement load balancer function**

In `src/core/server.rs`:

```rust
async fn select_agent_for_group(
    db: &CoreDb,
    registry: &ConnectionRegistry,
    group: &TaskGroup,
    new_task_count: i64,
    heartbeat_timeout_seconds: i64,
) -> Result<Option<String>> {
    let mut candidate_ids = if let Some(force_agent_id) = &group.force_agent_id {
        vec![force_agent_id.clone()]
    } else {
        group.candidate_ids.clone()
    };

    if candidate_ids.len() == 1 {
        if let Ok(group_id) = candidate_ids[0].parse::<i64>() {
            let expanded = db.expand_agent_group(group_id).await?;
            if !expanded.is_empty() {
                candidate_ids = expanded;
            }
        }
    }

    let candidates = db.list_dispatch_candidates(&candidate_ids).await?;
    let now = chrono::Local::now().naive_local();
    let mut best: Option<(String, f64)> = None;

    for candidate in candidates {
        if candidate.agent_isuse_flag != 1 {
            continue;
        }
        let agent_id = candidate.agent_id.to_string();
        if !registry.is_connected(&agent_id).await {
            continue;
        }
        if candidate.current_status.as_deref() != Some("ONLINE") {
            continue;
        }
        let Some(last_heartbeat_time) = candidate.last_heartbeat_time.as_deref() else { continue; };
        let heartbeat_time = chrono::NaiveDateTime::parse_from_str(last_heartbeat_time, "%Y-%m-%d %H:%M:%S")?;
        if (now - heartbeat_time).num_seconds() > heartbeat_timeout_seconds {
            continue;
        }
        let load_limit = candidate.host_load_limit.unwrap_or(90.0);
        if candidate.cpu_load.unwrap_or(0.0) >= load_limit || candidate.memory_load.unwrap_or(0.0) >= load_limit {
            continue;
        }
        let running = db.count_active_tasks_by_agent(&agent_id).await?;
        let power = candidate.agent_power.unwrap_or(1.0).max(1.0);
        if running + new_task_count > power.floor() as i64 {
            continue;
        }
        let score = score_agent(power, running, new_task_count, 1.0);
        if best.as_ref().map(|(_, current)| score > *current).unwrap_or(true) {
            best = Some((agent_id, score));
        }
    }

    Ok(best.map(|(agent_id, _)| agent_id))
}
```

- [ ] **Step 8: Run focused tests**

Run: `cargo test expand_agent_group_returns_member_ids` and `cargo test score_agent_prefers_more_available_capacity`

Expected: PASS.

- [ ] **Step 9: Commit checkpoint**

Skip this step unless the user explicitly asks to commit. If requested, run:

```bash
git add src/core_agent_api.rs src/core/db.rs src/core/server.rs
git commit -m "feat: add dispatch candidate load balancing"
```

---

### Task 5: Route Immediate Strategies Through TaskGroup Dispatch

**Files:**
- Modify: `src/core/server.rs`

**Interfaces:**
- Consumes: `TaskGroup`, `compute_task_group_id`, `select_agent_for_group`, `CoreDb::create_task`, `CoreDb::assign_group_to_agent`, `CoreDb::update_group_status`.
- Produces: `async fn dispatch_strategy_command(state: &CoreState, command: StrategyCommand) -> Result<bool>`.

- [ ] **Step 1: Write failing test for command-to-group construction**

Add a pure helper test in `src/core/server.rs` tests:

```rust
    #[test]
    fn parse_candidate_ids_accepts_json_array() {
        let ids = parse_candidate_ids("[\"100\",\"200\"]").unwrap();
        assert_eq!(ids, vec!["100".to_string(), "200".to_string()]);
    }
```

- [ ] **Step 2: Implement candidate parser**

In `src/core/server.rs`:

```rust
fn parse_candidate_ids(raw: &str) -> Result<Vec<String>> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Vec<String>>(raw) {
        Ok(ids) => Ok(ids),
        Err(_) => Ok(raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect()),
    }
}
```

- [ ] **Step 3: Add request builder helper**

In `src/core/server.rs`, extract the current `TaskDispatchRequest` literal from `dispatch_for_strategy` into:

```rust
fn build_task_dispatch_request(
    strategy: &CollectionStrategyRow,
    unit: &DataCollectorUnitRow,
    config_snapshot_id: &str,
    group_id: &str,
    task_id: String,
    collect_id: String,
    logical_task_key: String,
    scan_start_time: String,
) -> TaskDispatchRequest {
    TaskDispatchRequest {
        task_id,
        logical_task_key,
        strategy_id: strategy.id.to_string(),
        group_id: Some(group_id.to_string()),
        config_snapshot_id: config_snapshot_id.to_string(),
        scan_start_time,
        collect_id,
        load_type: unit.load_type.clone(),
        encoding: unit.file_encoding.clone(),
        output_delimiter: unit.output_delimiter.clone(),
        timeout_seconds: unit.task_timeout_seconds as u64,
        table_name: strategy.table_name.clone(),
        source_type: unit.source_type.clone(),
        remote_pattern: unit.remote_pattern.clone(),
        source_host: unit.host.clone(),
        source_port: unit.port as u16,
        source_username: unit.username.clone(),
        source_password: unit.password.clone(),
        source_connect_retry: unit.connect_retry as u64,
        source_download_retry: unit.download_retry as u64,
        source_download_parallel: unit.download_parallel as u64,
        source_retry_interval_secs: unit.retry_interval_secs as u64,
        source_connect_timeout_secs: unit.connect_timeout_secs as u64,
        source_read_timeout_secs: unit.read_timeout_secs as u64,
        source_cache_retention_days: unit.cache_retention_days as u64,
        db_host: unit.db_host.clone(),
        db_port: unit.db_port as u16,
        db_user: unit.db_user.clone(),
        db_password: unit.db_password.clone(),
        db_database: unit.db_database.clone(),
        db_table_name_case: unit.db_table_name_case.clone(),
    }
}
```

- [ ] **Step 4: Implement `dispatch_strategy_command`**

Add in `src/core/server.rs`:

```rust
async fn dispatch_strategy_command(state: &CoreState, command: StrategyCommand) -> Result<bool> {
    if command.table_names.is_empty() {
        anyhow::bail!("strategy command has no table names");
    }
    let mut candidate_ids = parse_candidate_ids(&command.strategy.agent_ids)?;
    if candidate_ids.is_empty() {
        candidate_ids = parse_candidate_ids(&command.unit.agent_ids)?;
    }
    if candidate_ids.is_empty() {
        anyhow::bail!("strategy command has no candidate agents");
    }

    let strategy_id = command.strategy.id.to_string();
    let group_id = compute_task_group_id(
        &strategy_id,
        command.unit.id,
        &command.scan_start_time,
        command.scan_end_time.as_deref(),
        &command.table_names,
    );
    let group = TaskGroup {
        group_id: group_id.clone(),
        source: command.source.clone(),
        strategy_ids: vec![strategy_id.clone()],
        collector_id: command.unit.id,
        collector_name: command.unit.unit_name.clone(),
        candidate_ids,
        scan_start_time: command.scan_start_time.clone(),
        scan_end_time: command.scan_end_time.clone(),
        table_names: command.table_names.clone(),
        config_snapshot_id: command.config_snapshot_id.clone(),
        force_agent_id: command.force_agent_id.clone(),
        retry_count: 0,
    };

    let now = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let mut requests = Vec::new();
    for table_name in &group.table_names {
        let mut strategy = command.strategy.clone();
        strategy.table_name = table_name.clone();
        let task_id = format!("task_{}_{}_{}", strategy_id, table_name, now);
        let collect_id = format!("collect_{}_{}_{}", strategy_id, table_name, now);
        let logical_task_key = format!("strategy_{}:{}:{}", strategy_id, group.scan_start_time, table_name);
        let request = build_task_dispatch_request(
            &strategy,
            &command.unit,
            &group.config_snapshot_id,
            &group.group_id,
            task_id.clone(),
            collect_id.clone(),
            logical_task_key.clone(),
            group.scan_start_time.clone(),
        );
        state.db.create_task(
            &task_id,
            &logical_task_key,
            &strategy_id,
            &group.config_snapshot_id,
            &group.scan_start_time,
            &collect_id,
            "",
            &group.group_id,
        ).await?;
        requests.push(request);
    }

    let Some(agent_id) = select_agent_for_group(&state.db, &state.registry, &group, requests.len() as i64, 150).await? else {
        state.db.increment_group_retry(&group.group_id, &chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(), "no available agent").await?;
        return Ok(false);
    };

    state.db.assign_group_to_agent(&group.group_id, &agent_id).await?;
    state.db.update_group_status(&group.group_id, "DISPATCHING", None).await?;
    for request in requests {
        state.to_tcp.send((agent_id.clone(), InternalMessage::DispatchTask(request))).await?;
    }
    Ok(true)
}
```

- [ ] **Step 5: Replace immediate branch in `create_strategies`**

In `create_strategies`, replace the loop that calls `dispatch_for_strategy` with command construction:

```rust
        for row in &rows {
            let command = StrategyCommand {
                source: StrategyCommandSource::Immediate,
                strategy: row.clone(),
                unit: unit.clone(),
                config_snapshot_id: config_snapshot_id.clone(),
                scan_start_time: row.data_start_time.clone().unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
                scan_end_time: row.data_end_time.clone(),
                table_names: vec![row.table_name.clone()],
                force_agent_id: None,
            };
            match dispatch_strategy_command(&state, command).await {
                Ok(true) => tracing::info!("[create_strategies] dispatched strategy_id={}", row.id),
                Ok(false) => tracing::warn!("[create_strategies] queued retry for strategy_id={}", row.id),
                Err(e) => tracing::error!("[create_strategies] dispatch failed for strategy_id={}: {e}", row.id),
            }
        }
```

- [ ] **Step 6: Keep old `dispatch_for_strategy` temporarily or remove it**

If no call sites remain, remove `dispatch_for_strategy`. If `/api/tasks/dispatch` still uses direct dispatch, leave it for Task 7.

- [ ] **Step 7: Run focused tests**

Run: `cargo test parse_candidate_ids_accepts_json_array` and `cargo test task_group_id_is_stable_when_table_order_changes`

Expected: PASS.

- [ ] **Step 8: Compile all server tests**

Run: `cargo test list_agents_returns_ok`

Expected: PASS.

- [ ] **Step 9: Commit checkpoint**

Skip this step unless the user explicitly asks to commit. If requested, run:

```bash
git add src/core/server.rs
git commit -m "feat: dispatch immediate strategies via task groups"
```

---

### Task 6: Add Periodic Scan Loop With Duplicate Prevention

**Files:**
- Modify: `src/core/db.rs`
- Modify: `src/core/server.rs`

**Interfaces:**
- Consumes: `dispatch_strategy_command` from Task 5.
- Produces:
  - `CoreDb::list_active_periodic_strategies() -> Result<Vec<CollectionStrategyRow>>`
  - `CoreDb::task_exists_by_logical_key(logical_task_key: &str) -> Result<bool>`
  - `periodic_strategy_scan_loop(state: CoreState)`
  - `retry_dispatch_loop(state: CoreState)`

- [ ] **Step 1: Add DB method for active periodic strategies**

In `src/core/db.rs`:

```rust
    pub async fn list_active_periodic_strategies(&self) -> Result<Vec<CollectionStrategyRow>> {
        trace_sql!("SELECT id, collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at FROM collection_strategy WHERE strategy_type = 'periodic' AND status = '可用'");
        sqlx::query_as::<_, CollectionStrategyRow>(
            "SELECT id, collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at FROM collection_strategy WHERE strategy_type = 'periodic' AND status = '可用'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }
```

- [ ] **Step 2: Add DB method to prevent duplicate periodic tasks**

In `src/core/db.rs`:

```rust
    pub async fn task_exists_by_logical_key(&self, logical_task_key: &str) -> Result<bool> {
        trace_sql!("SELECT COUNT(*) FROM collect_tasks WHERE logical_task_key = ?", logical_task_key = logical_task_key);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collect_tasks WHERE logical_task_key = ?")
            .bind(logical_task_key)
            .fetch_one(&self.pool)
            .await?;
        Ok(count > 0)
    }
```

- [ ] **Step 3: Add CoreState clone requirement check**

Confirm `CoreState` remains `#[derive(Clone)]`. The scan and retry loops need cloned state.

- [ ] **Step 4: Implement periodic scanner loop**

In `src/core/server.rs`:

```rust
async fn periodic_strategy_scan_loop(state: CoreState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;
        let strategies = match state.db.list_active_periodic_strategies().await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(error = %e, "periodic strategy scan failed");
                continue;
            }
        };
        for strategy in strategies {
            let unit = match state.db.get_unit_by_id(strategy.collector_id).await {
                Ok(Some(unit)) => unit,
                Ok(None) => {
                    tracing::warn!(strategy_id = %strategy.id, collector_id = %strategy.collector_id, "periodic strategy unit not found");
                    continue;
                }
                Err(e) => {
                    tracing::warn!(strategy_id = %strategy.id, error = %e, "periodic strategy unit query failed");
                    continue;
                }
            };
            let config_snapshot_id = match state.db.get_active_snapshot_id_for_config_name(&unit.config_name).await {
                Ok(Some(id)) => id,
                Ok(None) => {
                    tracing::warn!(strategy_id = %strategy.id, config_name = %unit.config_name, "periodic strategy active snapshot not found");
                    continue;
                }
                Err(e) => {
                    tracing::warn!(strategy_id = %strategy.id, error = %e, "periodic strategy snapshot query failed");
                    continue;
                }
            };
            let scan_start_time = chrono::Local::now().format("%Y-%m-%d %H:%M:00").to_string();
            let logical_task_key = format!("strategy_{}:{}:{}", strategy.id, scan_start_time, strategy.table_name);
            match state.db.task_exists_by_logical_key(&logical_task_key).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(strategy_id = %strategy.id, error = %e, "periodic duplicate check failed");
                    continue;
                }
            }
            let command = StrategyCommand {
                source: StrategyCommandSource::Periodic,
                strategy: strategy.clone(),
                unit,
                config_snapshot_id,
                scan_start_time,
                scan_end_time: strategy.data_end_time.clone(),
                table_names: vec![strategy.table_name.clone()],
                force_agent_id: None,
            };
            if let Err(e) = dispatch_strategy_command(&state, command).await {
                tracing::warn!(strategy_id = %strategy.id, error = %e, "periodic strategy dispatch failed");
            }
        }
    }
}
```

- [ ] **Step 5: Spawn loops in `run_core_server`**

After `tcp_cleanup_loop` spawn:

```rust
    tokio::spawn(periodic_strategy_scan_loop(state.clone()));
```

- [ ] **Step 6: Run compile test**

Run: `cargo test list_agents_returns_ok`

Expected: PASS.

- [ ] **Step 7: Commit checkpoint**

Skip this step unless the user explicitly asks to commit. If requested, run:

```bash
git add src/core/db.rs src/core/server.rs
git commit -m "feat: add periodic strategy scan loop"
```

---

### Task 7: Update TCP Status Handling And Agent Ack

**Files:**
- Modify: `src/core/db.rs`
- Modify: `src/core/server.rs`
- Modify: `src/agent/server.rs`

**Interfaces:**
- Consumes: existing `InternalMessage::DispatchTaskAck(TaskDispatchResponse)` and `InternalMessage::TaskEvent(TaskEventRequest)`.
- Produces:
  - `CoreDb::update_task_status(task_id: &str, status: &str, error_message: Option<&str>) -> Result<u64>`
  - Agent sends `DispatchTaskAck` after accepting a task.

- [ ] **Step 1: Implement task status update method**

In `src/core/db.rs`:

```rust
    pub async fn update_task_status(&self, task_id: &str, status: &str, error_message: Option<&str>) -> Result<u64> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("UPDATE collect_tasks SET status = ?, last_progress_at = ?, dispatch_error = ? WHERE task_id = ?", status = status, task_id = task_id, error_message = error_message);
        let result = sqlx::query(
            "UPDATE collect_tasks SET status = ?, last_progress_at = ?, dispatch_error = ? WHERE task_id = ?",
        )
        .bind(status)
        .bind(&now)
        .bind(error_message)
        .bind(task_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
```

- [ ] **Step 2: Handle DispatchTaskAck in Core**

In `tcp_dispatch_loop` in `src/core/server.rs`, add a match arm:

```rust
            InternalMessage::DispatchTaskAck(ack) => {
                let status = if ack.accepted { "ACCEPTED" } else { "FAILED" };
                if let Err(e) = db.update_task_status(&ack.task_id, status, ack.reason.as_deref()).await {
                    tracing::warn!(%agent_id, task_id = %ack.task_id, error = %e, "update task ack status failed");
                }
            }
```

- [ ] **Step 3: Handle TaskEvent status in Core**

Replace the current `TaskEvent` logging-only arm with:

```rust
            InternalMessage::TaskEvent(event) => {
                tracing::info!(%agent_id, task_id = %event.event_id, status = ?event.status, phase = ?event.phase, "TaskEvent");
                let status = match event.status {
                    TaskStatus::Running => Some("RUNNING"),
                    TaskStatus::Failed => Some("FAILED"),
                    TaskStatus::Timeout => Some("TIMEOUT"),
                    TaskStatus::Cancelled => Some("CANCELLED"),
                    _ => None,
                };
                if let Some(status) = status {
                    if let Err(e) = db.update_task_status(&event.event_id, status, event.message.as_deref()).await {
                        tracing::warn!(%agent_id, task_id = %event.event_id, error = %e, "update task event status failed");
                    }
                }
            }
```

Ensure `TaskStatus` is imported in `src/core/server.rs`.

- [ ] **Step 4: Send Ack from Agent**

In `src/agent/server.rs`, inside `InternalMessage::DispatchTask(request) =>`, before spawning the task, add:

```rust
                let ack = crate::core_agent_api::TaskDispatchResponse {
                    task_id: request.task_id.clone(),
                    accepted: true,
                    agent_task_state: crate::core_agent_api::TaskStatus::Accepted,
                    reason: None,
                };
                if let Err(e) = send_to_tcp_tx.send(InternalMessage::DispatchTaskAck(ack)).await {
                    tracing::warn!(task_id = %request.task_id, error = %e, "failed to send dispatch ack");
                }
```

- [ ] **Step 5: Run compile test**

Run: `cargo test test_internal_message_roundtrip`

Expected: PASS.

- [ ] **Step 6: Commit checkpoint**

Skip this step unless the user explicitly asks to commit. If requested, run:

```bash
git add src/core/db.rs src/core/server.rs src/agent/server.rs
git commit -m "feat: update task status from agent events"
```

---

### Task 8: Fail Active Tasks On Heartbeat Timeout

**Files:**
- Modify: `src/core/server.rs`

**Interfaces:**
- Consumes: `CoreDb::mark_active_tasks_failed_for_agent(agent_id, reason)` from Task 2.
- Produces: heartbeat timeout marks Agent offline and fails active tasks.

- [ ] **Step 1: Modify cleanup loop**

In `tcp_cleanup_loop`, after `db.mark_agent_offline(agent_id_i64).await`, add:

```rust
                if let Err(e) = db.mark_active_tasks_failed_for_agent(&agent_id, "agent heartbeat timeout").await {
                    tracing::error!(%agent_id, error = %e, "mark active tasks failed after heartbeat timeout failed");
                }
```

- [ ] **Step 2: Run focused DB test from Task 2**

Run: `cargo test active_task_count_and_agent_failure_ignore_terminal_tasks`

Expected: PASS.

- [ ] **Step 3: Run compile test**

Run: `cargo test list_agents_returns_ok`

Expected: PASS.

- [ ] **Step 4: Commit checkpoint**

Skip this step unless the user explicitly asks to commit. If requested, run:

```bash
git add src/core/server.rs
git commit -m "feat: fail active tasks on agent heartbeat timeout"
```

---

### Task 9: Full Verification And Cleanup

**Files:**
- Modify only files already touched if compiler or tests reveal issues.

**Interfaces:**
- Consumes: all previous tasks.
- Produces: verified working implementation.

- [ ] **Step 1: Run full Rust test suite**

Run: `cargo test`

Expected: all tests pass.

- [ ] **Step 2: Fix compile or test failures without changing scope**

If `cargo test` fails, inspect the specific error and only fix issues caused by this plan. Do not refactor unrelated code. Common expected fixes:

```rust
// If a TaskDispatchRequest literal misses group_id:
group_id: None,
```

```rust
// If TaskStatus import is missing in src/core/server.rs:
use crate::core_agent_api::TaskStatus;
```

```rust
// If sqlx::Row is missing in a test module:
use sqlx::Row;
```

- [ ] **Step 3: Inspect changed files**

Run: `git diff -- src/core/db.rs src/core/server.rs src/core_agent_api.rs src/agent/server.rs docs/superpowers/plans/2026-07-08-strategy-dispatch-pipeline.md docs/superpowers/specs/2026-07-08-strategy-dispatch-pipeline-design.md`

Expected: diff contains only dispatch pipeline, task group metadata, status handling, and the docs created for this work.

- [ ] **Step 4: Optional release build**

Run only if requested or before delivery: `cargo build --release`

Expected: build succeeds.

- [ ] **Step 5: Commit final checkpoint**

Skip this step unless the user explicitly asks to commit. If requested, run:

```bash
git add src/core/db.rs src/core/server.rs src/core_agent_api.rs src/agent/server.rs docs/superpowers/specs/2026-07-08-strategy-dispatch-pipeline-design.md docs/superpowers/plans/2026-07-08-strategy-dispatch-pipeline.md
git commit -m "feat: add strategy dispatch pipeline"
```

---

## Self-Review Notes

- Spec coverage: the plan covers `group_id` self-association, Core-side task grouping, load-balanced Agent selection, direct immediate strategy replacement, periodic scanning with duplicate prevention, TCP sender path reuse, Agent ack, task status updates, and heartbeat failure handling.
- Scoped deferral: full durable retry rehydration, backfill scanning, and `DispatchTaskGroup` are intentionally deferred because first-stage design keeps TCP single-task dispatch.
- Placeholder scan: no implementation step relies on undefined placeholder work; deferred work is explicitly outside this implementation stage.
- Type consistency: `group_id` is `String`/`&str`, `agent_id` remains stringified CRC64 at dispatch boundaries, and DB methods return `Result<u64>` for update row counts or `Result<i64>` for counts.
