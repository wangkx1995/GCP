# Auto-Dispatch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add DB connection params to data_collector_unit, auto-dispatch tasks on immediate strategy creation, and pass source/load config inline to agents.

**Architecture:** Add columns to `data_collector_unit` table → update Rust structs → add form fields → extend `TaskDispatchRequest` with connection params → modify agent runner to construct config structs from params instead of files → wire auto-dispatch into `create_strategies` handler.

**Tech Stack:** Rust (axum, sqlx, SQLite), React 18 + Ant Design 5 + Vite

## Global Constraints

- SQLite ALTER TABLE for migration (no drop/create)
- `remote_file_source` crate types made `pub` (currently `pub(crate)`)
- `ParseJobOptions.source_config` changes from `Option<PathBuf>` to `Option<remote_file_source::SourceConfig>`
- `ParseJobOptions.load_config` changes from `PathBuf` to `LoadConfig`
- `TaskDispatchRequest` extends with all source + DB connection fields
- `DataCollectorUnitSaveRequest` load fields are `Option<T>` for partial update compatibility
- All 47 existing tests must pass

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/core/db.rs` | `init_db` migration + `get_unit_by_id` query |
| `src/core_agent_api.rs` | Updated structs (unit row, save request, task dispatch request) |
| `src/core/server.rs` | Auto-dispatch logic in `create_strategies` |
| `src/parse_job.rs` | `ParseJobOptions` struct field types |
| `src/agent/runner.rs` | Construct `SourceConfig`/`LoadConfig` from request params |
| `src/load_config.rs` | No changes (structs stay the same, just constructed differently) |
| `crates/remote-file-source/src/config.rs` | Visibility `pub(crate)` → `pub` |
| `crates/remote-file-source/src/lib.rs` | `ResolveOptions.source_config` type change |
| `pm-admin/src/types/api.ts` | `DataCollectorUnit` interface |
| `pm-admin/src/pages/DataCollectorUnits/FormPage.tsx` | New form fields |

---

### Task 1: DB Migration + Backend Types

**Files:**
- Modify: `src/core/db.rs` (init_db migration)
- Modify: `src/core_agent_api.rs` (all three structs)
- Create: (none)

**Interfaces:**
- Consumes: existing `Database` pool in `init_db`
- Produces: `DataCollectorUnitRow` with new fields, `DataCollectorUnitSaveRequest` with new Option fields, `TaskDispatchRequest` with new fields, `get_unit_by_id(id: i64) -> Result<DataCollectorUnitRow>`

- [ ] **Step 1: Add migration SQL**

In `src/core/db.rs`, inside `init_db`, add ALTER TABLE statements after the existing table creation:

```rust
// ── Auto-dispatch columns ──
sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN load_type TEXT NOT NULL DEFAULT 'clickhouse'")
    .execute(&self.pool).await.ok();
sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN output_delimiter TEXT NOT NULL DEFAULT '|'")
    .execute(&self.pool).await.ok();
sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_host TEXT NOT NULL DEFAULT ''")
    .execute(&self.pool).await.ok();
sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_port INTEGER NOT NULL DEFAULT 9000")
    .execute(&self.pool).await.ok();
sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_user TEXT NOT NULL DEFAULT ''")
    .execute(&self.pool).await.ok();
sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_password TEXT NOT NULL DEFAULT ''")
    .execute(&self.pool).await.ok();
sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_database TEXT NOT NULL DEFAULT ''")
    .execute(&self.pool).await.ok();
sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_table_name_case TEXT NOT NULL DEFAULT 'lower'")
    .execute(&self.pool).await.ok();
```

- [ ] **Step 2: Update DataCollectorUnitRow**

In `src/core_agent_api.rs`, add to the struct (after `cache_retention_days`):

```rust
    pub load_type: String,
    pub output_delimiter: String,
    pub db_host: String,
    pub db_port: i64,
    pub db_user: String,
    pub db_password: String,
    pub db_database: String,
    pub db_table_name_case: String,
```

- [ ] **Step 3: Update DataCollectorUnitSaveRequest**

Add matching `Option<T>` fields:

```rust
    pub load_type: Option<String>,
    pub output_delimiter: Option<String>,
    pub db_host: Option<String>,
    pub db_port: Option<i64>,
    pub db_user: Option<String>,
    pub db_password: Option<String>,
    pub db_database: Option<String>,
    pub db_table_name_case: Option<String>,
```

- [ ] **Step 4: Update TaskDispatchRequest**

Add ALL connection params (source + DB). Insert after `callback_base_url`:

```rust
    // Source connection (was source.toml)
    pub source_type: String,
    pub remote_pattern: String,
    pub source_host: String,
    pub source_port: u16,
    pub source_username: String,
    pub source_password: String,
    pub source_connect_retry: u64,
    pub source_download_retry: u64,
    pub source_download_parallel: u64,
    pub source_retry_interval_secs: u64,
    pub source_connect_timeout_secs: u64,
    pub source_read_timeout_secs: u64,
    pub source_cache_retention_days: u64,
    // DB connection (was load.toml)
    pub db_host: String,
    pub db_port: u16,
    pub db_user: String,
    pub db_password: String,
    pub db_database: String,
    pub db_table_name_case: String,
```

- [ ] **Step 5: Add get_unit_by_id query**

In `src/core/db.rs`, after `list_data_collector_units`:

```rust
    pub async fn get_unit_by_id(&self, id: i64) -> Result<Option<DataCollectorUnitRow>> {
        let row = sqlx::query_as::<_, DataCollectorUnitRow>(
            "SELECT id, unit_name, config_name, config_version, table_names, agent_ids, data_interval_seconds, collector_interval, task_timeout_seconds, source_type, file_encoding, remote_pattern, host, port, username, password, connect_retry, download_retry, download_parallel, retry_interval_secs, connect_timeout_secs, read_timeout_secs, cache_retention_days, load_type, output_delimiter, db_host, db_port, db_user, db_password, db_database, db_table_name_case, created_at, updated_at FROM data_collector_unit WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }
```

- [ ] **Step 6: Add callback_base_url to CoreState**

In `src/core/server.rs`, add `callback_base_url` field:

```rust
pub struct CoreState {
    pub db: CoreDb,
    pub http: reqwest::Client,
    pub storage: Arc<ConfigStorage>,
    pub callback_base_url: String,
}
```

In `run_core_server`, set it when constructing state:

```rust
    let callback_base_url = format!("http://{addr}/api");
    let state = CoreState {
        db: CoreDb::open(db_path).await?,
        http: reqwest::Client::new(),
        storage: Arc::new(storage),
        callback_base_url,
    };
```

- [ ] **Step 8: Build and test**

```bash
cargo test --lib 2>&1 | tail -5
```

Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "feat: add DB connection columns to data_collector_unit and update Rust types"
```

---

### Task 2: Frontend — Add Form Fields

**Files:**
- Modify: `pm-admin/src/types/api.ts`
- Modify: `pm-admin/src/pages/DataCollectorUnits/FormPage.tsx`

**Interfaces:**
- Consumes: `DataCollectorUnit` interface from Task 1
- Produces: form fields for load_type, output_delimiter, db_* params

- [ ] **Step 1: Update TypeScript interface**

In `pm-admin/src/types/api.ts`, add to `DataCollectorUnit`:

```ts
  load_type: string;
  output_delimiter: string;
  db_host: string;
  db_port: number;
  db_user: string;
  db_password: string;
  db_database: string;
  db_table_name_case: string;
```

Add to `DataCollectorUnitSaveRequest`:

```ts
  load_type?: string;
  output_delimiter?: string;
  db_host?: string;
  db_port?: number;
  db_user?: string;
  db_password?: string;
  db_database?: string;
  db_table_name_case?: string;
```

- [ ] **Step 2: Add form fields in FormPage.tsx**

In the form, after the existing "采集配置" section (after `task_timeout_seconds`), add a new Divider "入库配置" with these fields:

```tsx
<Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>入库配置</Divider>
<div style={{ display: 'flex', gap: 16 }}>
  <div style={{ flex: 1 }}>
    <Form.Item name="load_type" label="入库类型" rules={[{ required: true }]}>
      <Select options={[
        { value: 'clickhouse', label: 'ClickHouse' },
        { value: 'postgresql', label: 'PostgreSQL' },
      ]} />
    </Form.Item>
  </div>
  <div style={{ flex: 1 }}>
    <Form.Item name="output_delimiter" label="输出分隔符" rules={[{ required: true }]}>
      <Select options={[
        { value: '|', label: '竖线 |' },
        { value: ',', label: '逗号 ,' },
        { value: '\t', label: '制表符 \\t' },
      ]} />
    </Form.Item>
  </div>
  <div style={{ flex: 1 }}>
    <Form.Item name="db_table_name_case" label="表名大小写" initialValue="lower">
      <Select options={[
        { value: 'lower', label: '小写' },
        { value: 'upper', label: '大写' },
      ]} />
    </Form.Item>
  </div>
</div>
<Form.Item name="db_host" label="数据库地址" rules={[{ required: true }]}>
  <Input placeholder="127.0.0.1" />
</Form.Item>
<div style={{ display: 'flex', gap: 16 }}>
  <div style={{ width: 120 }}>
    <Form.Item name="db_port" label="端口" rules={[{ required: true }]}>
      <InputNumber style={{ width: '100%' }} min={1} max={65535} placeholder="9000" />
    </Form.Item>
  </div>
  <div style={{ flex: 1 }}>
    <Form.Item name="db_user" label="数据库用户" rules={[{ required: true }]}>
      <Input placeholder="default" />
    </Form.Item>
  </div>
  <div style={{ flex: 1 }}>
    <Form.Item name="db_password" label="数据库密码">
      <Input.Password placeholder="可选" />
    </Form.Item>
  </div>
  <div style={{ flex: 1 }}>
    <Form.Item name="db_database" label="数据库名" rules={[{ required: true }]}>
      <Input placeholder="default" />
    </Form.Item>
  </div>
</div>
```

Also add initial values for `load_type: 'clickhouse'`, `output_delimiter: '|'`, `db_table_name_case: 'lower'` in the form's `initialValues`.

In handleSave, add to the `DataCollectorUnitSaveRequest`:

```ts
load_type: values.load_type,
output_delimiter: values.output_delimiter,
db_host: values.db_host,
db_port: values.db_port,
db_user: values.db_user,
db_password: values.db_password || undefined,
db_database: values.db_database,
db_table_name_case: values.db_table_name_case,
```

- [ ] **Step 3: Build**

```bash
source ~/.nvm/nvm.sh && nvm use 22 && npm run build
```

Expected: Build succeeds.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: add DB connection fields to data_collector_unit form"
```

---

### Task 3: remote_file_source — Make Types Public, Update ResolveOptions

**Files:**
- Modify: `crates/remote-file-source/src/config.rs`
- Modify: `crates/remote-file-source/src/lib.rs`

**Interfaces:**
- Consumes: existing `SourceConfig`, `ResolveOptions`
- Produces: `SourceConfig` with `pub` visibility, `ResolveOptions.source_config` changed to `Option<SourceConfig>`

- [ ] **Step 1: Make types public in config.rs**

Change visibility:
- `SourceConfig` struct: `pub(crate)` → `pub`
- `SourceSection` struct: `pub(crate)` → `pub`
- `SourceKind` enum: `pub(crate)` → `pub`
- `ConnectionConfig` struct: `pub(crate)` → `pub`
- All fields within these types: `pub(crate)` → `pub`

- [ ] **Step 2: Update ResolveOptions in lib.rs**

Change `source_config` field type:
```rust
    pub source_config: Option<config::SourceConfig>,
```

- [ ] **Step 3: Update resolve_routed_files_with_router**

Change the `(None, Some(config_path))` branch to use the parsed config directly instead of calling `load_source_config`:

```rust
(None, Some(config)) => {
    let scan_start_time = options
        .scan_start_time
        .as_deref()
        .context("--scan-start-time is required when --source-config is used")?;
    resolve_remote_files(config, scan_start_time, &route_remote_file)
}
```

- [ ] **Step 4: Update tests**

In `config.rs` tests, no changes needed (tests are in the same crate, visibility doesn't affect them).

In `lib.rs` tests, remove or update any tests that passed a `PathBuf` to `source_config`.

- [ ] **Step 5: Build**

```bash
cd crates/remote-file-source && cargo test
```

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "refactor: make SourceConfig public, change ResolveOptions to use parsed config"
```

---

### Task 4: Agent — Construct Configs from Params

**Files:**
- Modify: `src/parse_job.rs`
- Modify: `src/agent/runner.rs`

**Interfaces:**
- Consumes: `TaskDispatchRequest` from Task 1, `SourceConfig`/`SourceSection`/`ConnectionConfig`/`SourceKind` from Task 3, `LoadConfig` from `src/load_config.rs`
- Produces: Updated `ParseJobOptions` with inline config types

- [ ] **Step 1: Update ParseJobOptions**

Change two field types:

```rust
pub struct ParseJobOptions {
    pub input: Option<PathBuf>,
    pub source_config: Option<remote_file_source::SourceConfig>,  // was Option<PathBuf>
    pub scan_start_time: Option<String>,
    pub config_dir: PathBuf,
    pub output_dir: PathBuf,
    pub collect_id: String,
    pub load_type: LoadType,
    pub load_config: LoadConfig,  // was PathBuf
    pub output_delimiter: String,
    pub encoding: String,
    pub recursive: bool,
    pub rule_files: Vec<PathBuf>,
    pub rules_dir: Option<PathBuf>,
}
```

(Remove the `use crate::load_config::LoadConfig;` if redundant)

- [ ] **Step 2: Update run_parse_job**

Change the load_config reading and source_config passing:

```rust
    // Before:
    let load_config = crate::load_config::load_config(&options.load_config)
        .with_context(|| format!("failed to parse {}", options.load_config.display()))?;

    // After:
    let load_config = &options.load_config;

    // ───

    // Before (in resolve_routed_files_with_router call):
            source_config: options.source_config,

    // After (unchanged — the type changed, but the field name and usage are the same)
```

- [ ] **Step 3: Construct SourceConfig in AgentRunner**

In `src/agent/runner.rs`, before building `ParseJobOptions`, construct configs from task params:

```rust
use remote_file_source::{SourceConfig, SourceSection, SourceKind, ConnectionConfig};
use crate::load_config::{LoadConfig, ClickHouseConfig, PostgresConfig};

// ── after determining load_type ──

let source_config = SourceConfig {
    source: SourceSection {
        kind: match task.source_type.to_ascii_lowercase().as_str() {
            "ftp" => SourceKind::Ftp,
            _ => SourceKind::Sftp,
        },
        download_dir: task_dir.join("downloads"),
        remote_pattern: task.remote_pattern.clone(),
        cache_retention_days: task.source_cache_retention_days,
        connect_retry: task.source_connect_retry as usize,
        download_retry: task.source_download_retry as usize,
        download_parallel: task.source_download_parallel as usize,
        retry_interval_secs: task.source_retry_interval_secs,
        connect_timeout_secs: task.source_connect_timeout_secs,
        read_timeout_secs: task.source_read_timeout_secs,
        connection: ConnectionConfig {
            host: task.source_host.clone(),
            port: task.source_port,
            username: task.source_username.clone(),
            password: task.source_password.clone(),
        },
    },
};

let load_config = match load_type {
    LoadType::Clickhouse => LoadConfig {
        clickhouse: ClickHouseConfig {
            client: "clickhouse-client".to_string(),
            host: task.db_host.clone(),
            port: task.db_port,
            user: task.db_user.clone(),
            password: task.db_password.clone(),
            database: task.db_database.clone(),
            table_name_case: task.db_table_name_case.clone(),
        },
        postgresql: PostgresConfig {
            client: "psql".to_string(),
            host: String::new(),
            port: 5432,
            user: String::new(),
            password: String::new(),
            database: String::new(),
        },
    },
    LoadType::Postgresql => LoadConfig {
        clickhouse: ClickHouseConfig {
            client: "clickhouse-client".to_string(),
            host: String::new(),
            port: 9000,
            user: String::new(),
            password: String::new(),
            database: String::new(),
            table_name_case: "lower".to_string(),
        },
        postgresql: PostgresConfig {
            client: "psql".to_string(),
            host: task.db_host.clone(),
            port: task.db_port,
            user: task.db_user.clone(),
            password: task.db_password.clone(),
            database: task.db_database.clone(),
        },
    },
};
```

- [ ] **Step 4: Update ParseJobOptions construction in runner**

Replace the relevant part:

```rust
        let opts = ParseJobOptions {
            input: None,  // remote mode, no local input
            source_config: Some(source_config),
            scan_start_time: Some(task.scan_start_time.clone()),
            config_dir: config_dir.clone(),
            output_dir: output_dir.clone(),
            collect_id: task.collect_id.clone(),
            load_type,
            load_config,  // now a LoadConfig struct, not a PathBuf
            output_delimiter: task.output_delimiter.clone(),
            encoding: task.encoding.clone(),
            recursive: false,
            rule_files: Vec::new(),
            rules_dir: Some(config_dir.join("rules")),
        };
```

- [ ] **Step 5: Remove file-based use_remote check**

The runner no longer needs to check if `source.toml` exists. Remote mode is always true when source config is present.

```rust
        // Remove:
        // let source_toml = config_dir.join("source.toml");
        // let use_remote = source_toml.exists();
        // tracing::info!("[agent] source mode: {} (source.toml exists={})", ...);
```

- [ ] **Step 6: Build and run agent tests**

```bash
cargo test --lib
```

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "refactor: construct SourceConfig/LoadConfig from task params in agent runner"
```

---

### Task 5: Backend — Auto-Dispatch in create_strategies

**Files:**
- Modify: `src/core/server.rs`
- Modify: `src/core/db.rs` (if new query needed)

**Interfaces:**
- Consumes: `get_unit_by_id` from Task 1, existing `select_online_agent`, `create_task`, `dispatch_task` logic
- Produces: Auto-dispatch after strategy creation

- [ ] **Step 1: Add get_active_snapshot_for_config_name query to db.rs**

```rust
    pub async fn get_active_snapshot_id_for_config_name(&self, config_name: &str) -> Result<Option<String>> {
        sqlx::query_scalar(
            "SELECT config_snapshot_id FROM config_snapshots WHERE name = ? AND is_active = 1 ORDER BY created_at DESC LIMIT 1"
        )
        .bind(config_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }
```

- [ ] **Step 2: Add dispatch_for_strategy helper to db.rs or server.rs**

This is a private async fn that dispatches one strategy row:

```rust
use crate::core_agent_api::TaskDispatchRequest;

async fn dispatch_for_strategy(
    state: &CoreState,
    strategy: &CollectionStrategyRow,
    unit: &DataCollectorUnitRow,
    config_snapshot_id: &str,
) -> Result<bool> {
    let now = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let strategy_id = strategy.id.to_string();
    let scan_start_time = strategy.execute_time.clone()
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
    let task_id = format!("task_immediate_{}_{}", strategy_id, now);
    let collect_id = format!("collect_immediate_{}_{}", strategy_id, now);
    let logical_task_key = format!("strategy_{}:{}", strategy_id, scan_start_time);

    let request = TaskDispatchRequest {
        task_id: task_id.clone(),
        logical_task_key,
        strategy_id,
        config_snapshot_id: config_snapshot_id.to_string(),
        scan_start_time,
        collect_id,
        load_type: unit.load_type.clone(),
        encoding: unit.file_encoding.clone(),
        output_delimiter: unit.output_delimiter.clone(),
        timeout_seconds: unit.task_timeout_seconds as u64,
        callback_base_url: state.callback_base_url.clone(),
        // Source params
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
        // DB params
        db_host: unit.db_host.clone(),
        db_port: unit.db_port as u16,
        db_user: unit.db_user.clone(),
        db_password: unit.db_password.clone(),
        db_database: unit.db_database.clone(),
        db_table_name_case: unit.db_table_name_case.clone(),
    };

    // Select agent
    let (agent_id, agent_host, agent_port) = state.db.select_online_agent().await?;
    // Create task in DB
    state.db.create_task(
        &task_id,
        &request.logical_task_key,
        &request.strategy_id,
        &request.config_snapshot_id,
        &request.scan_start_time,
        &request.collect_id,
        &agent_id,
    ).await?;
    // Forward to agent
    let agent_url = format!("http://{agent_host}:{agent_port}/api/tasks");
    let agent_resp = state.http.post(&agent_url).json(&request).send().await?;
    let accepted = agent_resp.status().is_success();
    Ok(accepted)
}
```

- [ ] **Step 3: Modify create_strategies handler in server.rs**

After successfully inserting strategies, add dispatch for immediate strategies:

```rust
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

    let rows = match state.db.create_strategies(&req).await {
        Ok(rows) => rows,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("DB错误: {e}")).into_response(),
    };

    // Auto-dispatch for immediate strategies
    if req.strategy_type == "immediate" {
        let unit = match state.db.get_unit_by_id(req.collector_id).await {
            Ok(Some(u)) => u,
            Ok(None) => {
                tracing::warn!("[create_strategies] unit not found for collector_id={}", req.collector_id);
                return ok_response(rows, "策略已创建，但采集单元不存在").into_response();
            }
            Err(e) => {
                tracing::warn!("[create_strategies] failed to get unit: {e}");
                return ok_response(rows, &format!("策略已创建，但查询采集单元失败: {e}")).into_response();
            }
        };
        let config_snapshot_id = match state.db.get_active_snapshot_id_for_config_name(&unit.config_name).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                tracing::warn!("[create_strategies] no active snapshot for config_name={}", unit.config_name);
                return ok_response(rows, "策略已创建，但未找到激活的配置快照").into_response();
            }
            Err(e) => {
                tracing::warn!("[create_strategies] failed to get snapshot: {e}");
                return ok_response(rows, &format!("策略已创建，但查询快照失败: {e}")).into_response();
            }
        };

        for row in &rows {
            match dispatch_for_strategy(&state, row, &unit, &config_snapshot_id).await {
                Ok(true) => tracing::info!("[create_strategies] dispatched strategy_id={}", row.id),
                Ok(false) => tracing::warn!("[create_strategies] agent rejected strategy_id={}", row.id),
                Err(e) => tracing::error!("[create_strategies] dispatch failed for strategy_id={}: {e}", row.id),
            }
        }
    }

    ok_response(rows, "创建成功").into_response()
}
```

Note: You'll also need a `ok_response_with_warning` helper or modify the response to include warnings. A simple approach: add a `warning` field to the response object, or just log and return the rows with a message.

- [ ] **Step 4: Build**

```bash
cargo test --lib
```

Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: auto-dispatch tasks on immediate strategy creation"
```

---

### Task 6: Integration Test + Final Verification

**Files:**
- Create: (test added to existing `src/core/db.rs` tests)
- Modify: `src/core/server.rs` tests (if applicable)

- [ ] **Step 1: Run full test suite**

```bash
cargo test --lib 2>&1 | tail -5
```

Expected: All tests pass.

- [ ] **Step 2: Build frontend**

```bash
source ~/.nvm/nvm.sh && nvm use 22 && npm run build 2>&1 | tail -3
```

Expected: Build succeeds.

- [ ] **Step 3: Commit any remaining changes**

```bash
git add -A && git commit -m "test: verify auto-dispatch and frontend build"
```
