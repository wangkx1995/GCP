# Data Collector Unit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `data_collector_unit` management — backend CRUD + HTTP endpoints + frontend page.

**Architecture:** Add table migration in `CoreDb::init_schema()`, CRUD methods in `CoreDb`, 6 new HTTP endpoints in `CoreServer`, new types in `core_agent_api.rs`, and a full-page form on the existing `AgentConfig` frontend route.

**Tech Stack:** Rust (sqlx, axum), TypeScript (React 18, Ant Design 5, TanStack Query, axios)

## Global Constraints

- All DB schema changes are additive (CREATE TABLE IF NOT EXISTS, ALTER TABLE only)
- Response format: `ApiResponse<T>` wrapper with `{ data, status, message }` (auto-unwrapped by frontend axios interceptor)
- Password handling: list returns `"******"`, save with empty/`"******"` means keep existing
- Time format: `"YYYY-MM-DD HH:mm:ss"` (chrono::Local::now().format)
- SQL logging pattern: `tracing::debug!` with SQL text + `tracing::debug!` with Parameters
- `config_version` auto-populated from active snapshot's `config_snapshot_id` for given `config_name`

---

### Task 1: DB Migration + CoreDb CRUD Methods

**Files:**
- Modify: `src/core_agent_api.rs` — add new types
- Modify: `src/core/db.rs` — add table + methods + tests

---

### Task 2: Backend HTTP Endpoints

**Files:**
- Modify: `src/core/server.rs` — add 6 endpoints + register routes
- Already modified: `src/core_agent_api.rs` (from Task 1)

---

### Task 3: Frontend API + Types + Hooks

**Files:**
- Modify: `pm-admin/src/types/api.ts` — add TypeScript interfaces
- Create: `pm-admin/src/api/data-collector-units.ts` — API functions
- Modify: `pm-admin/src/api/hooks.ts` — add TanStack Query hooks

---

### Task 4: Frontend AgentConfig Page

**Files:**
- Modify: `pm-admin/src/pages/AgentConfig/index.tsx` — full implementation

---

### Task 5: Update API Docs

**Files:**
- Modify: `docs/frontend-api-docs.md` — add 6 new endpoints + types

---

## Task Details

### Task 1: DB Migration + CoreDb CRUD Methods

**Interfaces:**
- Consumes: nothing (new types + table)
- Produces: `CoreDb` methods `next_unit_id`, `list_data_collector_units`, `upsert_data_collector_unit`, `delete_data_collector_unit`, `search_active_config_names`, `tables_for_config`. Types `DataCollectorUnitRow`, `DataCollectorUnitSaveRequest`, `NextIdResponse`, `ConfigNameItem`, `ConfigNamesResponse`, `TablesResponse` in `core_agent_api.rs`.

- [ ] **Step 1: Add types to `core_agent_api.rs`**

Add after `ConfigUpdateRequest` struct:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataCollectorUnitRow {
    pub id: i64,
    pub unit_name: String,
    pub config_name: String,
    pub config_version: String,
    pub table_names: String,
    pub agent_ids: String,
    pub data_interval_seconds: i64,
    pub collector_interval: i64,
    pub task_timeout_seconds: i64,
    pub source_type: String,
    pub file_encoding: String,
    pub remote_pattern: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub password: String,
    pub connect_retry: i64,
    pub download_retry: i64,
    pub download_parallel: i64,
    pub retry_interval_secs: i64,
    pub connect_timeout_secs: i64,
    pub read_timeout_secs: i64,
    pub cache_retention_days: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DataCollectorUnitSaveRequest {
    pub unit_name: String,
    pub config_name: String,
    pub table_names: String,
    pub agent_ids: String,
    pub data_interval_seconds: Option<i64>,
    pub collector_interval: Option<i64>,
    pub task_timeout_seconds: Option<i64>,
    pub source_type: Option<String>,
    pub file_encoding: Option<String>,
    pub remote_pattern: Option<String>,
    pub host: Option<String>,
    pub port: Option<i64>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub connect_retry: Option<i64>,
    pub download_retry: Option<i64>,
    pub download_parallel: Option<i64>,
    pub retry_interval_secs: Option<i64>,
    pub connect_timeout_secs: Option<i64>,
    pub read_timeout_secs: Option<i64>,
    pub cache_retention_days: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NextIdResponse {
    pub id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigNameItem {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigNamesResponse {
    pub config_names: Vec<ConfigNameItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TablesResponse {
    pub tables: Vec<String>,
}
```

Also add import for the new types in the `use` block at `db.rs` line 7-10:

```rust
use crate::core_agent_api::{
    AgentCapabilities, AgentInfo, AgentRegisterRequest, AgentStatus, ConfigSnapshotMeta,
    ConfigSnapshotResponse, DataCollectorUnitRow, DataCollectorUnitSaveRequest,
    OnlineAgent, ResultRow, TaskResultReport, TaskStatus,
};
```

- [ ] **Step 2: Add table creation in `init_schema()`**

After the `config_tables` table creation (after line 143), add:

```rust
sqlx::query(
    r#"
    CREATE TABLE IF NOT EXISTS data_collector_unit (
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
    )
    "#,
)
.execute(&self.pool)
.await?;
```

- [ ] **Step 3: Add `next_unit_id` method**

After `result_rows_for_day`, add (after line 559):

```rust
pub async fn next_unit_id(&self) -> Result<i64> {
    tracing::debug!("[db] ==> SELECT COALESCE(MAX(id), 0) + 1 FROM data_collector_unit");
    let row: (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(id), 0) + 1 FROM data_collector_unit",
    )
    .fetch_one(&self.pool)
    .await?;
    Ok(row.0)
}
```

- [ ] **Step 4: Add `list_data_collector_units` method**

```rust
pub async fn list_data_collector_units(&self) -> Result<Vec<DataCollectorUnitRow>> {
    tracing::debug!("[db] ==> SELECT * FROM data_collector_unit ORDER BY id DESC");
    let rows = sqlx::query_as::<_, DataCollectorUnitRow>(
        "SELECT id, unit_name, config_name, config_version, table_names, agent_ids, \
         data_interval_seconds, collector_interval, task_timeout_seconds, \
         source_type, file_encoding, remote_pattern, host, port, username, password, \
         connect_retry, download_retry, download_parallel, retry_interval_secs, \
         connect_timeout_secs, read_timeout_secs, cache_retention_days, \
         created_at, updated_at \
         FROM data_collector_unit ORDER BY id DESC",
    )
    .fetch_all(&self.pool)
    .await?;
    // Mask passwords for all rows
    let rows = rows.into_iter().map(|mut r| {
        r.password = "******".to_string();
        r
    }).collect();
    Ok(rows)
}
```

Note: To use `sqlx::query_as` for `DataCollectorUnitRow`, we need `sqlx::FromRow` derive. Add `sqlx::FromRow` to the derive list for `DataCollectorUnitRow`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DataCollectorUnitRow {
```

- [ ] **Step 5: Add `upsert_data_collector_unit` method**

This method validates references, handles password preservation, auto-populates config_version, and sets timestamps.

```rust
pub async fn upsert_data_collector_unit(
    &self,
    id: i64,
    data: &DataCollectorUnitSaveRequest,
) -> Result<()> {
    // Validate config_name exists in active snapshots
    let config_exists: bool = sqlx::query_scalar::<_, i32>(
        "SELECT COUNT(*) FROM config_snapshots WHERE name = ? AND is_active = 1",
    )
    .bind(&data.config_name)
    .fetch_one(&self.pool)
    .await? != 0;
    if !config_exists {
        anyhow::bail!("config_name '{}' not found or not active", data.config_name);
    }

    // Validate agent_ids: parse JSON array, check each exists
    let agent_ids: Vec<String> = serde_json::from_str(&data.agent_ids)
        .map_err(|_| anyhow::anyhow!("agent_ids is not a valid JSON array"))?;
    for aid in &agent_ids {
        let agent_exists: bool = sqlx::query_scalar::<_, i32>(
            "SELECT COUNT(*) FROM agents WHERE agent_id = ?",
        )
        .bind(aid)
        .fetch_one(&self.pool)
        .await? != 0;
        if !agent_exists {
            anyhow::bail!("agent_id '{}' not found", aid);
        }
    }

    let now = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    // Resolve password: if empty or "******", keep existing
    let password = match &data.password {
        p if p.is_empty() || p == "******" => {
            let existing: Option<String> = sqlx::query_scalar(
                "SELECT password FROM data_collector_unit WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .unwrap_or_default();
            existing
        }
        p => p.clone(),
    };

    // Resolve config_version from active snapshot
    let config_version: String = sqlx::query_scalar(
        "SELECT config_snapshot_id FROM config_snapshots WHERE name = ? AND is_active = 1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&data.config_name)
    .fetch_optional(&self.pool)
    .await?
    .unwrap_or_default();

    // Check if row exists (for created_at preservation)
    let existing_created: Option<String> = sqlx::query_scalar(
        "SELECT created_at FROM data_collector_unit WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&self.pool)
    .await?;

    let created_at = existing_created.unwrap_or_else(|| now.clone());

    tracing::debug!("[db] ==> INSERT OR REPLACE INTO data_collector_unit(...) VALUES(?)");
    sqlx::query(
        r#"
        INSERT OR REPLACE INTO data_collector_unit(
            id, unit_name, config_name, config_version, table_names, agent_ids,
            data_interval_seconds, collector_interval, task_timeout_seconds,
            source_type, file_encoding, remote_pattern, host, port, username, password,
            connect_retry, download_retry, download_parallel, retry_interval_secs,
            connect_timeout_secs, read_timeout_secs, cache_retention_days,
            created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id)
    .bind(&data.unit_name)
    .bind(&data.config_name)
    .bind(&config_version)
    .bind(&data.table_names)
    .bind(&data.agent_ids)
    .bind(data.data_interval_seconds.unwrap_or(900))
    .bind(data.collector_interval.unwrap_or(900))
    .bind(data.task_timeout_seconds.unwrap_or(3600))
    .bind(data.source_type.as_deref().unwrap_or("sftp"))
    .bind(data.file_encoding.as_deref().unwrap_or("UTF-8"))
    .bind(data.remote_pattern.as_deref().unwrap_or(""))
    .bind(data.host.as_deref().unwrap_or(""))
    .bind(data.port.unwrap_or(22))
    .bind(data.username.as_deref().unwrap_or(""))
    .bind(&password)
    .bind(data.connect_retry.unwrap_or(3))
    .bind(data.download_retry.unwrap_or(3))
    .bind(data.download_parallel.unwrap_or(4))
    .bind(data.retry_interval_secs.unwrap_or(30))
    .bind(data.connect_timeout_secs.unwrap_or(30))
    .bind(data.read_timeout_secs.unwrap_or(300))
    .bind(data.cache_retention_days.unwrap_or(7))
    .bind(&created_at)
    .bind(&now)
    .execute(&self.pool)
    .await?;

    Ok(())
}
```

- [ ] **Step 6: Add `delete_data_collector_unit` method**

```rust
pub async fn delete_data_collector_unit(&self, id: i64) -> Result<bool> {
    tracing::debug!("[db] ==> DELETE FROM data_collector_unit WHERE id=?");
    tracing::debug!("[db] ==> Parameters: id={}", id);
    let result = sqlx::query("DELETE FROM data_collector_unit WHERE id = ?")
        .bind(id)
        .execute(&self.pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 7: Add `search_active_config_names` method**

```rust
pub async fn search_active_config_names(&self, search: Option<&str>) -> Result<Vec<ConfigNameItem>> {
    match search {
        Some(q) if !q.is_empty() => {
            let pattern = format!("%{}%", q);
            tracing::debug!("[db] ==> SELECT DISTINCT name,config_snapshot_id FROM config_snapshots WHERE is_active=1 AND name LIKE ? ORDER BY name");
            let rows = sqlx::query_as::<_, ConfigNameItem>(
                "SELECT DISTINCT name, config_snapshot_id FROM config_snapshots WHERE is_active = 1 AND name LIKE ? ORDER BY name",
            )
            .bind(&pattern)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows)
        }
        _ => {
            tracing::debug!("[db] ==> SELECT DISTINCT name,config_snapshot_id FROM config_snapshots WHERE is_active=1 ORDER BY name");
            let rows = sqlx::query_as::<_, ConfigNameItem>(
                "SELECT DISTINCT name, config_snapshot_id FROM config_snapshots WHERE is_active = 1 ORDER BY name",
            )
            .fetch_all(&self.pool)
            .await?;
            Ok(rows)
        }
    }
}
```

Note: Add `sqlx::FromRow` derive to `ConfigNameItem`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConfigNameItem {
    pub name: String,
    pub version: String,
}
```

And add a `#[sqlx(rename = "config_snapshot_id")]` annotation or map the field:

Actually, `ConfigNameItem` has fields `name` and `version` but the SQL selects `name` and `config_snapshot_id`. So we need to either:
1. Alias in SQL: `SELECT DISTINCT name, config_snapshot_id AS version FROM ...`
2. Or add `#[sqlx(rename = "config_snapshot_id")]` on `version`

Let me use option 1 (aliasing in SQL) to keep the type clean:

```rust
let rows = sqlx::query_as::<_, ConfigNameItem>(
    "SELECT DISTINCT name, config_snapshot_id AS version FROM config_snapshots WHERE is_active = 1 AND name LIKE ? ORDER BY name",
)
```

- [ ] **Step 8: Add `tables_for_config` method**

```rust
pub async fn tables_for_config(&self, config_name: &str) -> Result<Vec<String>> {
    tracing::debug!("[db] ==> SELECT DISTINCT ct.table_name FROM config_tables ct INNER JOIN config_snapshots cs ON ct.config_snapshot_id = cs.config_snapshot_id WHERE cs.name = ? ORDER BY ct.table_name");
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT ct.table_name FROM config_tables ct \
         INNER JOIN config_snapshots cs ON ct.config_snapshot_id = cs.config_snapshot_id \
         WHERE cs.name = ? ORDER BY ct.table_name",
    )
    .bind(config_name)
    .fetch_all(&self.pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 9: Build and run tests to verify compilation**

Run: `cargo build 2>&1`

Expected: compilation succeeds

- [ ] **Step 10: Write tests for CRUD methods**

After the existing tests in `mod tests` at `db.rs:562`, add:

```rust
#[tokio::test]
async fn data_collector_unit_crud() {
    let db = db().await;

    // next_unit_id returns 1 on empty table
    let id = db.next_unit_id().await.unwrap();
    assert_eq!(id, 1);

    // next_unit_id after insert returns next
    let save = DataCollectorUnitSaveRequest {
        unit_name: "test-unit".to_string(),
        config_name: "test-config".to_string(),
        table_names: "[\"t1\"]".to_string(),
        agent_ids: "[]".to_string(),
        data_interval_seconds: Some(900),
        collector_interval: Some(900),
        task_timeout_seconds: Some(3600),
        source_type: Some("sftp".to_string()),
        file_encoding: Some("UTF-8".to_string()),
        remote_pattern: Some("/path/{scan_start_time}".to_string()),
        host: Some("192.168.1.1".to_string()),
        port: Some(22),
        username: Some("user".to_string()),
        password: Some("pass".to_string()),
        connect_retry: Some(3),
        download_retry: Some(3),
        download_parallel: Some(4),
        retry_interval_secs: Some(30),
        connect_timeout_secs: Some(30),
        read_timeout_secs: Some(300),
        cache_retention_days: Some(7),
    };
    // Upsert will fail because config_name doesn't exist
    let result = db.upsert_data_collector_unit(1, &save).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found or not active"));

    // Insert a config snapshot first
    use crate::core_agent_api::RuleFile;
    db.insert_config_snapshot(&ConfigSnapshotResponse {
        config_snapshot_id: "v_test".to_string(),
        content_hash: "sha256:test".to_string(),
        source_toml: "".to_string(),
        mapping_dx_ini: "".to_string(),
        load_toml: "".to_string(),
        col_name_cut_config_ini: None,
        rules: vec![RuleFile {
            relative_path: "rules/a.json".to_string(),
            content: "{\"table_name\":\"t1\"}".to_string(),
        }],
    }).await.unwrap();
    // Activate it with name = test-config
    db.insert_config_snapshot_meta("v_test", "sha256:test", "v_test", 1, "test-config", &["t1".to_string()]).await.unwrap();
    // Activate
    db.activate_config_snapshot("v_test").await.unwrap();

    // Now upsert should succeed
    db.upsert_data_collector_unit(1, &save).await.unwrap();

    // next_unit_id returns 2
    let id2 = db.next_unit_id().await.unwrap();
    assert_eq!(id2, 2);

    // List returns 1 item
    let list = db.list_data_collector_units().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].unit_name, "test-unit");
    assert_eq!(list[0].password, "******"); // masked

    // Delete
    let deleted = db.delete_data_collector_unit(1).await.unwrap();
    assert!(deleted);
    let list = db.list_data_collector_units().await.unwrap();
    assert_eq!(list.len(), 0);

    // Delete non-existent returns false
    let deleted = db.delete_data_collector_unit(999).await.unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn search_config_names_and_tables() {
    let db = db().await;

    // Insert test config snapshots
    db.insert_config_snapshot_meta("v1", "sha256:aaa", "v1", 1, "cfg-a", &["t1".to_string(), "t2".to_string()]).await.unwrap();
    db.insert_config_snapshot_meta("v2", "sha256:bbb", "v2", 1, "cfg-b", &["t3".to_string()]).await.unwrap();
    // Activate v1 only
    db.activate_config_snapshot("v1").await.unwrap();

    // Search without filter returns only active (cfg-a, v1)
    let names = db.search_active_config_names(None).await.unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].name, "cfg-a");
    assert_eq!(names[0].version, "v1");

    // Search with filter
    let names = db.search_active_config_names(Some("cfg")).await.unwrap();
    assert_eq!(names.len(), 1);

    // Tables for config
    let tables = db.tables_for_config("cfg-a").await.unwrap();
    assert_eq!(tables, vec!["t1".to_string(), "t2".to_string()]);
    let tables = db.tables_for_config("cfg-b").await.unwrap();
    assert!(tables.is_empty());
}
```

- [ ] **Step 11: Run tests to verify**

Run: `cargo test data_collector_unit -- --nocapture`
Expected: both new tests PASS

- [ ] **Step 12: Run full test suite**

Run: `cargo test`
Expected: all existing tests + new tests PASS

- [ ] **Step 13: Commit**

```bash
git add src/core_agent_api.rs src/core/db.rs
git commit -m "feat: add data_collector_unit table and CoreDb CRUD methods"
```

---

### Task 2: Backend HTTP Endpoints

**Files:**
- Modify: `src/core/server.rs` — add 6 endpoints + register routes

**Interfaces:**
- Consumes: `CoreDb` methods from Task 1, shared types from `core_agent_api.rs`
- Produces: HTTP endpoints for all 6 operations

- [ ] **Step 1: Add imports for new types in `server.rs`**

Expand the existing `use` block (lines 17-22):

```rust
use crate::core::config_storage::ConfigStorage;
use crate::core::db::CoreDb;
use crate::core_agent_api::{
    AgentRegisterRequest, AgentRegisterResponse, ConfigNameItem, ConfigNamesResponse,
    DataCollectorUnitRow, DataCollectorUnitSaveRequest, NextIdResponse,
    TablesResponse, TaskDispatchRequest, TaskDispatchResponse, TaskResultReport,
};
```

- [ ] **Step 2: Register routes in `router()` function**

After line 74 (before `.with_state(state)`), add:

```rust
        .route("/api/data-collector-units/next-id", post(next_unit_id))
        .route("/api/data-collector-units", get(list_data_collector_units))
        .route("/api/data-collector-units/:id", put(upsert_data_collector_unit))
        .route("/api/data-collector-units/:id", delete(delete_data_collector_unit_handler))
        .route("/api/data-collector-units/config-names", get(search_config_names))
        .route("/api/data-collector-units/tables", get(tables_for_config_handler))
```

Note: `put` and `delete` need to be imported from axum::routing:
```rust
use axum::routing::{get, post, put, delete};
```

- [ ] **Step 3: Add `next_unit_id` handler**

```rust
async fn next_unit_id(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Response {
    match state.db.next_unit_id().await {
        Ok(id) => ok_response(NextIdResponse { id }, "获取 ID 成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}
```

- [ ] **Step 4: Add `list_data_collector_units` handler**

```rust
async fn list_data_collector_units(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Response {
    match state.db.list_data_collector_units().await {
        Ok(list) => ok_response(list, "获取采集单元列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}
```

- [ ] **Step 5: Add `upsert_data_collector_unit` handler**

```rust
async fn upsert_data_collector_unit(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(data): Json<DataCollectorUnitSaveRequest>,
) -> Response {
    match state.db.upsert_data_collector_unit(id, &data).await {
        Ok(_) => ok_response(serde_json::json!({"id": id}), "保存成功").into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") || msg.contains("invalid") {
                err_response(StatusCode::BAD_REQUEST, msg).into_response()
            } else {
                err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response()
            }
        }
    }
}
```

- [ ] **Step 6: Add `delete_data_collector_unit_handler`**

```rust
async fn delete_data_collector_unit_handler(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Response {
    match state.db.delete_data_collector_unit(id).await {
        Ok(true) => ok_response(serde_json::json!({"deleted": true}), "删除成功").into_response(),
        Ok(false) => err_response(StatusCode::NOT_FOUND, format!("采集单元 {id} 不存在")).into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}
```

- [ ] **Step 7: Add `search_config_names` handler**

```rust
#[derive(serde::Deserialize)]
struct SearchQuery {
    search: Option<String>,
}

async fn search_config_names(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
) -> Response {
    match state.db.search_active_config_names(query.search.as_deref()).await {
        Ok(names) => ok_response(ConfigNamesResponse { config_names: names }, "获取配置名称列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}
```

- [ ] **Step 8: Add `tables_for_config_handler`**

```rust
#[derive(serde::Deserialize)]
struct ConfigNameQuery {
    config_name: String,
}

async fn tables_for_config_handler(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Query(query): axum::extract::Query<ConfigNameQuery>,
) -> Response {
    match state.db.tables_for_config(&query.config_name).await {
        Ok(tables) => ok_response(TablesResponse { tables }, "获取表名列表成功").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB 错误: {e}")).into_response(),
    }
}
```

- [ ] **Step 9: Build and test**

Run: `cargo build`
Expected: compilation succeeds

- [ ] **Step 10: Run tests**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 11: Commit**

```bash
git add src/core/server.rs
git commit -m "feat: add data_collector_unit HTTP endpoints"
```

---

### Task 3: Frontend API + Types + Hooks

**Files:**
- Modify: `pm-admin/src/types/api.ts`
- Create: `pm-admin/src/api/data-collector-units.ts`
- Modify: `pm-admin/src/api/hooks.ts`

**Interfaces:**
- Consumes: backend endpoint shapes from Task 2
- Produces: typed API functions + TanStack Query hooks

- [ ] **Step 1: Add types to `api.ts`**

Add before the last `export` (before line 135):

```typescript
export interface DataCollectorUnit {
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

export interface DataCollectorUnitSaveRequest {
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

export interface NextIdResponse {
  id: number;
}

export interface ConfigNameItem {
  name: string;
  version: string;
}

export interface ConfigNamesResponse {
  config_names: ConfigNameItem[];
}

export interface TablesResponse {
  tables: string[];
}
```

- [ ] **Step 2: Create `pm-admin/src/api/data-collector-units.ts`**

```typescript
import http from './client';
import type {
  DataCollectorUnit,
  DataCollectorUnitSaveRequest,
  NextIdResponse,
  ConfigNamesResponse,
  TablesResponse,
} from '../types/api';

export function listDataCollectorUnits() {
  return http.get<DataCollectorUnit[]>('/data-collector-units').then(r => r.data);
}

export function nextUnitId() {
  return http.post<NextIdResponse>('/data-collector-units/next-id').then(r => r.data);
}

export function saveDataCollectorUnit(id: number, data: DataCollectorUnitSaveRequest) {
  return http.put<{ id: number }>(`/data-collector-units/${id}`, data).then(r => r.data);
}

export function deleteDataCollectorUnit(id: number) {
  return http.delete<{ deleted: boolean }>(`/data-collector-units/${id}`).then(r => r.data);
}

export function searchConfigNames(search?: string) {
  const params = search ? { search } : {};
  return http.get<ConfigNamesResponse>('/data-collector-units/config-names', { params }).then(r => r.data);
}

export function getTablesForConfig(config_name: string) {
  return http.get<TablesResponse>('/data-collector-units/tables', { params: { config_name } }).then(r => r.data);
}
```

- [ ] **Step 3: Add hooks to `hooks.ts`**

Add after the `useAgents` hook:

```typescript
import {
  listDataCollectorUnits,
  nextUnitId,
  saveDataCollectorUnit,
  deleteDataCollectorUnit,
  searchConfigNames,
  getTablesForConfig,
} from './data-collector-units';
import type {
  DataCollectorUnitSaveRequest,
  ConfigNameItem,
} from '../types/api';

export function useDataCollectorUnits() {
  return useQuery({
    queryKey: ['data-collector-units'],
    queryFn: listDataCollectorUnits,
    refetchInterval: 30_000,
  });
}

export function useNextUnitId() {
  return useMutation({
    mutationFn: nextUnitId,
  });
}

export function useSaveDataCollectorUnit() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: number; data: DataCollectorUnitSaveRequest }) =>
      saveDataCollectorUnit(id, data),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['data-collector-units'] });
    },
  });
}

export function useDeleteDataCollectorUnit() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: deleteDataCollectorUnit,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['data-collector-units'] });
    },
  });
}

export function useConfigNames(search: string | undefined) {
  return useQuery({
    queryKey: ['config-names', search],
    queryFn: () => searchConfigNames(search),
    enabled: search !== undefined,
    staleTime: 60_000,
  });
}

export function useTablesForConfig(configName: string | undefined) {
  return useQuery({
    queryKey: ['config-tables', configName],
    queryFn: () => getTablesForConfig(configName!),
    enabled: !!configName,
    staleTime: 60_000,
  });
}
```

- [ ] **Step 4: Verify TypeScript compilation**

Run: `nvm use 22 && npx tsc --noEmit`
Expected: no type errors

- [ ] **Step 5: Commit**

```bash
git add pm-admin/src/types/api.ts pm-admin/src/api/data-collector-units.ts pm-admin/src/api/hooks.ts
git commit -m "feat: add data_collector_unit frontend API layer"
```

---

### Task 4: Frontend AgentConfig Page

**Files:**
- Modify: `pm-admin/src/pages/AgentConfig/index.tsx`

- [ ] **Step 1: Implement full AgentConfig page**

Replace the placeholder in `pm-admin/src/pages/AgentConfig/index.tsx` with:

```typescript
import { useState, useEffect, useCallback } from 'react';
import {
  Table, Card, Button, Form, Input, InputNumber, Select, message, Popconfirm,
  Space, Row, Col, DatePicker,
} from 'antd';
import { PlusOutlined, DeleteOutlined, SaveOutlined } from '@ant-design/icons';
import {
  useDataCollectorUnits,
  useNextUnitId,
  useSaveDataCollectorUnit,
  useDeleteDataCollectorUnit,
  useAgents,
  useConfigNames,
  useTablesForConfig,
} from '../../api/hooks';
import type { DataCollectorUnit, DataCollectorUnitSaveRequest } from '../../types/api';

export default function AgentConfigPage() {
  const { data: units, isLoading } = useDataCollectorUnits();
  const { data: agents } = useAgents();
  const nextIdMutation = useNextUnitId();
  const saveMutation = useSaveDataCollectorUnit();
  const deleteMutation = useDeleteDataCollectorUnit();

  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [editing, setEditing] = useState(false);
  const [form] = Form.useForm();

  const selectedUnit = units?.find(u => u.id === selectedId);

  // Config name search state
  const [configSearch, setConfigSearch] = useState<string>();
  const { data: configNamesData } = useConfigNames(configSearch);
  const configNames = configNamesData?.config_names ?? [];

  // Tables for selected config
  const watchedConfigName = Form.useWatch('config_name', form);
  const { data: tablesData } = useTablesForConfig(watchedConfigName);
  const availableTables = tablesData?.tables ?? [];

  // Reset form when selection changes
  useEffect(() => {
    if (selectedUnit && !editing) {
      const tableNames: string[] = tryParseJson(selectedUnit.table_names, []);
      const agentIdList: string[] = tryParseJson(selectedUnit.agent_ids, []);
      form.setFieldsValue({ ...selectedUnit, table_names: tableNames, agent_ids: agentIdList });
    } else if (!selectedUnit) {
      form.resetFields();
    }
  }, [selectedUnit, editing, form]);

  function tryParseJson(val: string, fallback: string[]) {
    try { return JSON.parse(val); } catch { return fallback; }
  }

  const handleNew = useCallback(async () => {
    const result = await nextIdMutation.mutateAsync();
    const newId = result.id;
    form.resetFields();
    form.setFieldsValue({ id: newId, collector_interval: 900, data_interval_seconds: 900 });
    setSelectedId(newId);
    setEditing(true);
  }, [nextIdMutation, form]);

  const handleSave = useCallback(async () => {
    const values = await form.validateFields();
    const id = values.id;
    const saveData: DataCollectorUnitSaveRequest = {
      unit_name: values.unit_name,
      config_name: values.config_name,
      table_names: JSON.stringify(values.table_names || []),
      agent_ids: JSON.stringify(values.agent_ids || []),
      data_interval_seconds: values.data_interval_seconds,
      collector_interval: values.collector_interval,
      task_timeout_seconds: values.task_timeout_seconds,
      source_type: values.source_type,
      file_encoding: values.file_encoding,
      remote_pattern: values.remote_pattern,
      host: values.host,
      port: values.port,
      username: values.username,
      password: values.password,
      connect_retry: values.connect_retry,
      download_retry: values.download_retry,
      download_parallel: values.download_parallel,
      retry_interval_secs: values.retry_interval_secs,
      connect_timeout_secs: values.connect_timeout_secs,
      read_timeout_secs: values.read_timeout_secs,
      cache_retention_days: values.cache_retention_days,
    };
    await saveMutation.mutateAsync({ id, data: saveData });
    message.success('保存成功');
    setEditing(false);
  }, [form, saveMutation]);

  const handleDelete = useCallback(async (id: number) => {
    await deleteMutation.mutateAsync(id);
    message.success('删除成功');
    if (selectedId === id) {
      setSelectedId(null);
    }
  }, [deleteMutation, selectedId]);

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '单元名称', dataIndex: 'unit_name', key: 'unit_name' },
    { title: '适配器名称', dataIndex: 'config_name', key: 'config_name' },
    { title: '适配器版本', dataIndex: 'config_version', key: 'config_version' },
    { title: '采集表', dataIndex: 'table_names', key: 'table_names', render: (v: string) => {
      try { return JSON.parse(v).join(', '); } catch { return v; }
    }},
    { title: '采集机', dataIndex: 'agent_ids', key: 'agent_ids', render: (v: string) => {
      try { return JSON.parse(v).join(', '); } catch { return v; }
    }},
    { title: '数据周期(秒)', dataIndex: 'data_interval_seconds', key: 'data_interval_seconds' },
    { title: '采集周期(秒)', dataIndex: 'collector_interval', key: 'collector_interval' },
    { title: '数据源', dataIndex: 'source_type', key: 'source_type' },
    { title: '主机', dataIndex: 'host', key: 'host' },
    { title: '端口', dataIndex: 'port', key: 'port' },
    { title: '用户名', dataIndex: 'username', key: 'username' },
    { title: '远程路径', dataIndex: 'remote_pattern', key: 'remote_pattern' },
    { title: '编码', dataIndex: 'file_encoding', key: 'file_encoding' },
    {
      title: '操作', key: 'action', width: 80,
      render: (_: unknown, record: DataCollectorUnit) => (
        <Popconfirm title="确认删除?" onConfirm={() => handleDelete(record.id)}>
          <Button danger size="small" icon={<DeleteOutlined />} loading={deleteMutation.isPending} />
        </Popconfirm>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <h2>采集单元配置</h2>
        <p>管理采集单元，绑定适配器、采集机、数据源和调度配置</p>
      </div>

      <Row gutter={16}>
        <Col span={24} lg={10}>
          <Card
            title="采集单元列表"
            className="content-card"
            styles={{ body: { padding: 0 } }}
            extra={
              <Button type="primary" icon={<PlusOutlined />} onClick={handleNew} loading={nextIdMutation.isPending}>
                新建
              </Button>
            }
          >
            <Table<DataCollectorUnit>
              className="data-table"
              rowKey="id"
              dataSource={units}
              columns={columns}
              loading={isLoading}
              pagination={false}
              scroll={{ x: 'max-content' }}
              size="small"
              onRow={(record) => ({
                onClick: () => { setSelectedId(record.id); setEditing(false); },
                style: { cursor: 'pointer', background: selectedId === record.id ? '#E6F4FF' : undefined },
              })}
            />
          </Card>
        </Col>
        <Col span={24} lg={14}>
          <Card
            title={selectedId ? `编辑采集单元 #${selectedId}` : '选择或新建采集单元'}
            className="content-card"
          >
            <Form
              form={form}
              layout="vertical"
              disabled={!editing && !!selectedUnit}
              initialValues={{ collector_interval: 900, data_interval_seconds: 900 }}
            >
              <Form.Item name="id" hidden><Input /></Form.Item>
              <Row gutter={16}>
                <Col span={12}>
                  <Form.Item name="unit_name" label="单元名称" rules={[{ required: true }]}>
                    <Input />
                  </Form.Item>
                </Col>
                <Col span={12}>
                  <Form.Item name="config_name" label="适配器名称" rules={[{ required: true }]}>
                    <Select
                      showSearch
                      onSearch={setConfigSearch}
                      filterOption={false}
                      options={configNames.map(n => ({ label: `${n.name} (${n.version})`, value: n.name }))}
                    />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={12}>
                  <Form.Item name="table_names" label="采集表" rules={[{ required: true }]}>
                    <Select
                      mode="multiple"
                      options={availableTables.map(t => ({ label: t, value: t }))}
                    />
                  </Form.Item>
                </Col>
                <Col span={12}>
                  <Form.Item name="agent_ids" label="采集机" rules={[{ required: true }]}>
                    <Select
                      mode="multiple"
                      options={(agents ?? []).map(a => ({ label: `${a.agent_name} (${a.agent_id})`, value: a.agent_id }))}
                    />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={8}>
                  <Form.Item name="data_interval_seconds" label="数据周期(秒)">
                    <InputNumber style={{ width: '100%' }} min={60} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="collector_interval" label="采集周期(秒)">
                    <InputNumber style={{ width: '100%' }} min={60} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="task_timeout_seconds" label="任务超时(秒)">
                    <InputNumber style={{ width: '100%' }} min={60} />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={8}>
                  <Form.Item name="source_type" label="数据源类型">
                    <Select options={[
                      { label: 'SFTP', value: 'sftp' },
                      { label: 'FTP', value: 'ftp' },
                    ]} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="file_encoding" label="文件编码">
                    <Input />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="remote_pattern" label="远程文件路径">
                    <Input />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={8}>
                  <Form.Item name="host" label="主机地址">
                    <Input />
                  </Form.Item>
                </Col>
                <Col span={4}>
                  <Form.Item name="port" label="端口">
                    <InputNumber style={{ width: '100%' }} min={1} max={65535} />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="username" label="用户名">
                    <Input />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="password" label="密码">
                    <Input.Password />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={6}>
                  <Form.Item name="connect_retry" label="连接重试">
                    <InputNumber style={{ width: '100%' }} min={0} />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="download_retry" label="下载重试">
                    <InputNumber style={{ width: '100%' }} min={0} />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="download_parallel" label="并行下载数">
                    <InputNumber style={{ width: '100%' }} min={1} />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="retry_interval_secs" label="重试间隔(秒)">
                    <InputNumber style={{ width: '100%' }} min={5} />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={8}>
                  <Form.Item name="connect_timeout_secs" label="连接超时(秒)">
                    <InputNumber style={{ width: '100%' }} min={5} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="read_timeout_secs" label="读取超时(秒)">
                    <InputNumber style={{ width: '100%' }} min={10} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="cache_retention_days" label="缓存保留(天)">
                    <InputNumber style={{ width: '100%' }} min={1} />
                  </Form.Item>
                </Col>
              </Row>
              <Space>
                {!editing && selectedUnit ? (
                  <Button type="primary" onClick={() => setEditing(true)}>编辑</Button>
                ) : (
                  <Button
                    type="primary"
                    icon={<SaveOutlined />}
                    onClick={handleSave}
                    loading={saveMutation.isPending}
                  >
                    保存
                  </Button>
                )}
              </Space>
            </Form>
          </Card>
        </Col>
      </Row>
    </div>
  );
}
```

- [ ] **Step 2: Verify TypeScript compilation**

Run: `nvm use 22 && npx tsc --noEmit`
Expected: no type errors

- [ ] **Step 3: Verify build**

Run: `npm run build`
Expected: build succeeds, outputs in `dist/`

- [ ] **Step 4: Commit**

```bash
git add pm-admin/src/pages/AgentConfig/index.tsx
git commit -m "feat: implement AgentConfig full page with list and form"
```

---

### Task 5: Update API Docs

**Files:**
- Modify: `docs/frontend-api-docs.md`

- [ ] **Step 1: Add new endpoints section**

After the existing section 5 (Agent 端点), insert a new section:

```markdown
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
```

Then update section numbers for subsequent sections (7. Results, 8. Types, 9. flows, etc.).

- [ ] **Step 2: Update the type definitions section**

Add new types to section 8 (types):

```typescript
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

- [ ] **Step 3: Commit**

```bash
git add docs/frontend-api-docs.md
git commit -m "docs: add data_collector_unit API endpoints"
```
