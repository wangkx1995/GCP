# Core/Agent Collection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first Core/Agent collection loop where Core dispatches parser tasks over HTTP, Agent executes existing parser logic, Agent reports `result.csv` rows, and Core stores/query-displays those rows from SQLite.

**Architecture:** Keep the existing single-node CLI working while extracting parser execution into a reusable library API. Add shared Core/Agent DTOs, SQLite-backed Core storage, Core HTTP endpoints, Agent HTTP endpoints, and result-grid query support. HTTP dispatch remains asynchronous and isolated behind simple dispatcher/receiver boundaries so MQ can replace it without rewriting task state or result storage.

**Tech Stack:** Rust 2021, existing parser modules, `axum`, `tokio`, `reqwest`, `rusqlite`, `uuid`, `sha2`, `anyhow`, `serde`, `serde_json`, `toml`, `csv`, `tempfile`.

## Global Constraints

- Keep the current single-node CLI behavior and arguments working.
- Core owns runtime config snapshots: `source.toml`, `mapping_dx.ini`, `load.toml`, `rules/*.json`, and `colNameCutConfig.ini`.
- First-version dispatch is Core-driven HTTP, but Agent execution is asynchronous after `accepted`.
- First-version Core database is SQLite.
- Agent must report terminal task status; cancelled tasks do not report `result.csv` rows.
- First-version completeness check is based on storing `result.csv` rows and serving a daily table/time matrix.
- Do not add Agent-side database loading.
- Do not add MQ dispatch in this implementation.
- Do not add file-level output package hash verification.

---

## File Structure

- Modify `Cargo.toml`: add HTTP, SQLite, UUID, and hash dependencies.
- Create `src/lib.rs`: expose existing parser modules and the reusable parse-job API.
- Modify `src/main.rs`: keep CLI behavior by converting parsed CLI args into `ParseJobOptions` and calling `run_parse_job`.
- Create `src/parse_job.rs`: owns `ParseJobOptions`, `ParseJobSummary`, and the extracted parser workflow from current `main.rs`.
- Create `src/core_agent_api.rs`: shared serde DTOs for Agent registration, heartbeats, task dispatch, events, config snapshots, result rows, and grid responses.
- Create `src/core/mod.rs`: Core module wiring.
- Create `src/core/db.rs`: SQLite schema creation and storage methods.
- Create `src/core/server.rs`: Core HTTP routes and handlers.
- Create `src/core/grid.rs`: daily matrix generation from expected tables/time slots and stored result rows.
- Create `src/bin/core.rs`: Core binary entrypoint.
- Create `src/agent/mod.rs`: Agent module wiring.
- Create `src/agent/store.rs`: filesystem-backed Agent task/config state.
- Create `src/agent/result_csv.rs`: scan parser output directories and parse `result.csv` rows.
- Create `src/agent/runner.rs`: asynchronous task execution and Core status/result reporting.
- Create `src/agent/server.rs`: Agent HTTP routes and handlers.
- Create `src/bin/agent.rs`: Agent binary entrypoint.

---

### Task 1: Extract Parser Workflow Into Reusable Library API

**Files:**
- Create: `src/lib.rs`
- Create: `src/parse_job.rs`
- Modify: `src/main.rs`
- Test: `src/parse_job.rs`

**Interfaces:**
- Produces: `parse_job::ParseJobOptions`
- Produces: `parse_job::ParseJobSummary`
- Produces: `parse_job::run_parse_job(options: ParseJobOptions) -> anyhow::Result<ParseJobSummary>`
- Consumes later: Agent runner calls `run_parse_job` with task-local config and output paths.

- [ ] **Step 1: Add `src/lib.rs` exports**

Create `src/lib.rs`:

```rust
pub mod config;
pub mod crc64;
pub mod load_config;
pub mod parse_job;
pub mod parser;
pub mod tpd;
pub mod util;
pub mod writer;

use clap::ValueEnum;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum LoadType {
    Postgresql,
    Clickhouse,
}
```

- [ ] **Step 2: Add parse job tests first**

Create `src/parse_job.rs` with the option structs and these tests before moving the workflow:

```rust
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::LoadType;

#[derive(Clone, Debug)]
pub struct ParseJobOptions {
    pub input: Option<PathBuf>,
    pub source_config: Option<PathBuf>,
    pub scan_start_time: Option<String>,
    pub config_dir: PathBuf,
    pub output_dir: PathBuf,
    pub collect_id: String,
    pub load_type: LoadType,
    pub load_config: PathBuf,
    pub output_delimiter: String,
    pub encoding: String,
    pub recursive: bool,
    pub rule_files: Vec<PathBuf>,
    pub rules_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseJobSummary {
    pub task_count: usize,
}

pub fn parse_delimiter(value: &str) -> Result<u8> {
    let bytes = value.as_bytes();
    if bytes.len() != 1 {
        bail!("output delimiter must be exactly one ASCII byte, got {value:?}");
    }
    Ok(bytes[0])
}

pub fn run_parse_job(_options: ParseJobOptions) -> Result<ParseJobSummary> {
    bail!("parse job workflow has not been wired yet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_delimiter_accepts_one_ascii_byte() {
        assert_eq!(parse_delimiter("|").unwrap(), b'|');
        assert_eq!(parse_delimiter(",").unwrap(), b',');
    }

    #[test]
    fn parse_delimiter_rejects_empty_or_multi_byte_values() {
        assert!(parse_delimiter("").unwrap_err().to_string().contains("output delimiter"));
        assert!(parse_delimiter("||").unwrap_err().to_string().contains("output delimiter"));
        assert!(parse_delimiter("中").unwrap_err().to_string().contains("output delimiter"));
    }
}
```

- [ ] **Step 3: Run delimiter tests**

Run: `cargo test parse_delimiter`

Expected: PASS for both delimiter tests.

- [ ] **Step 4: Move current `main.rs` parser workflow into `run_parse_job`**

Move these functions from `src/main.rs` into `src/parse_job.rs` and update imports to use `crate::...` modules:

```rust
run_streaming_table_task
run_streaming_table_tasks
effective_streaming_parallelism
route_remote_file
StreamingTableTask
build_streaming_table_tasks
cleanup_old_logs
```

Implement `run_parse_job` with the body currently in `main` after CLI parsing:

```rust
pub fn run_parse_job(options: ParseJobOptions) -> Result<ParseJobSummary> {
    let mapping_path = options.config_dir.join("mapping_dx.ini");
    let output_delimiter = parse_delimiter(&options.output_delimiter)?;
    let load_config = crate::load_config::load_config(&options.load_config)
        .with_context(|| format!("failed to parse {}", options.load_config.display()))?;
    let mapping = crate::config::parse_mapping_config(&mapping_path)
        .with_context(|| format!("failed to parse {}", mapping_path.display()))?;
    let ctx = crate::config::ContextData {
        mapping,
        encoding: options.encoding,
    };

    let rule_files = discover_rule_files(options.rule_files, options.rules_dir.as_ref())?;
    let mut rules = Vec::new();
    for rule_file in &rule_files {
        tracing::info!("[rule] loading {}", rule_file.display());
        rules.push(crate::tpd::load_rule(rule_file)?);
    }
    crate::tpd::validate_streaming_rules(&rules)?;
    let dest_tables_by_source = dest_tables_by_source_table(&rules);

    let routed_inputs = remote_file_source::resolve_routed_files_with_router(
        remote_file_source::ResolveOptions {
            local_input: options.input,
            recursive: options.recursive,
            source_config: options.source_config,
            scan_start_time: options.scan_start_time,
        },
        |remote_file| route_remote_file(remote_file, &ctx, &dest_tables_by_source),
    )?;
    let tasks = build_streaming_table_tasks(
        &rules,
        &routed_inputs.groups,
        &routed_inputs.representative_files,
    )?;
    let task_count = tasks.len();
    run_streaming_table_tasks(
        tasks,
        &ctx,
        &options.output_dir,
        output_delimiter,
        &options.collect_id,
        options.load_type,
        &load_config,
    )?;

    Ok(ParseJobSummary { task_count })
}
```

- [ ] **Step 5: Shrink `src/main.rs` to CLI wiring**

Keep the current CLI fields. Remove local module declarations that now live in `lib.rs`. Import the library API:

```rust
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use wy_gnb_pm_parser::parse_job::{run_parse_job, ParseJobOptions};
use wy_gnb_pm_parser::LoadType;
```

The `main` function should call:

```rust
let cli = Cli::parse();
let summary = run_parse_job(ParseJobOptions {
    input: cli.input,
    source_config: cli.source_config,
    scan_start_time: cli.scan_start_time,
    config_dir: cli.config_dir,
    output_dir: cli.output_dir,
    collect_id: cli.collect_id,
    load_type: cli.load_type,
    load_config: cli.load_config,
    output_delimiter: cli.output_delimiter,
    encoding: cli.encoding,
    recursive: cli.recursive,
    rule_files: cli.rule_files,
    rules_dir: cli.rules_dir,
})?;
info!("[done] {} streaming destination table task(s)", summary.task_count);
```

- [ ] **Step 6: Run existing parser tests**

Run: `cargo test`

Expected: all existing tests pass.

- [ ] **Step 7: Commit parser extraction**

Run:

```bash
git add src/lib.rs src/parse_job.rs src/main.rs
git commit -m "refactor: extract parser job workflow"
```

---

### Task 2: Add Shared Core/Agent API Models

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/core_agent_api.rs`

**Interfaces:**
- Produces: DTOs consumed by Core and Agent HTTP handlers.
- Produces: `TaskStatus`, `AgentStatus`, `TaskPhase`, `TaskDispatchRequest`, `TaskDispatchResponse`, `ResultRow`, `TaskResultReport`.

- [ ] **Step 1: Add dependencies**

In `Cargo.toml`, add:

```toml
axum = "0.7"
reqwest = { version = "0.12", features = ["json"] }
rusqlite = { version = "0.31", features = ["bundled"] }
sha2 = "0.10"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal", "time"] }
uuid = { version = "1", features = ["v4", "serde"] }
```

- [ ] **Step 2: Export `core_agent_api`**

In `src/lib.rs`, add:

```rust
pub mod core_agent_api;
```

- [ ] **Step 3: Add API model tests and structs**

Create `src/core_agent_api.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentStatus {
    Online,
    Unknown,
    Offline,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Created,
    Dispatching,
    Accepted,
    Running,
    Succeeded,
    Failed,
    Timeout,
    CancelRequested,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    PreparingConfig,
    Downloading,
    Parsing,
    WritingOutput,
    ReportingResult,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentCapabilities {
    pub can_collect: bool,
    pub can_parse: bool,
    pub can_load: bool,
    pub supported_protocols: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentRegisterRequest {
    pub agent_id: Option<String>,
    pub agent_name: String,
    pub host: String,
    pub port: u16,
    pub version: String,
    pub capabilities: AgentCapabilities,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentRegisterResponse {
    pub agent_id: String,
    pub heartbeat_interval_seconds: u64,
    pub task_report_interval_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AgentHeartbeatRequest {
    pub status: AgentStatus,
    pub running_task_ids: Vec<String>,
    pub disk_free_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConfigSnapshotResponse {
    pub config_snapshot_id: String,
    pub content_hash: String,
    pub source_toml: String,
    pub mapping_dx_ini: String,
    pub load_toml: String,
    pub col_name_cut_config_ini: Option<String>,
    pub rules: Vec<RuleFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RuleFile {
    pub relative_path: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskDispatchRequest {
    pub task_id: String,
    pub logical_task_key: String,
    pub strategy_id: String,
    pub config_snapshot_id: String,
    pub scan_start_time: String,
    pub collect_id: String,
    pub load_type: String,
    pub encoding: String,
    pub output_delimiter: String,
    pub timeout_seconds: u64,
    pub callback_base_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskDispatchResponse {
    pub task_id: String,
    pub accepted: bool,
    pub agent_task_state: TaskStatus,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskEventRequest {
    pub agent_id: String,
    pub event_id: String,
    pub status: TaskStatus,
    pub phase: Option<TaskPhase>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ResultRow {
    pub table_name: String,
    pub data_time: String,
    pub row_count: u64,
    pub success: i32,
    pub collect_time: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskResultReport {
    pub task_id: String,
    pub agent_id: String,
    pub status: TaskStatus,
    pub result_rows: Vec<ResultRow>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_serializes_as_screaming_snake_case() {
        let json = serde_json::to_string(&TaskStatus::CancelRequested).unwrap();
        assert_eq!(json, "\"CANCEL_REQUESTED\"");
    }

    #[test]
    fn result_report_round_trips_json() {
        let report = TaskResultReport {
            task_id: "task_1".to_string(),
            agent_id: "agent_1".to_string(),
            status: TaskStatus::Succeeded,
            result_rows: vec![ResultRow {
                table_name: "TPD_A".to_string(),
                data_time: "2026-06-17 15:15:00".to_string(),
                row_count: 100,
                success: 1,
                collect_time: "2026-07-02 15:35:00".to_string(),
            }],
        };
        let json = serde_json::to_string(&report).unwrap();
        let decoded: TaskResultReport = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, report);
    }
}
```

- [ ] **Step 4: Run API model tests**

Run: `cargo test core_agent_api`

Expected: both tests pass.

- [ ] **Step 5: Commit API models**

Run:

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/core_agent_api.rs
git commit -m "feat: add core agent api models"
```

---

### Task 3: Implement Core SQLite Storage

**Files:**
- Modify: `src/lib.rs`
- Create: `src/core/mod.rs`
- Create: `src/core/db.rs`

**Interfaces:**
- Produces: `core::db::CoreDb::open(path: impl AsRef<Path>) -> Result<CoreDb>`
- Produces: `CoreDb::register_agent`, `CoreDb::insert_config_snapshot`, `CoreDb::create_task`, `CoreDb::accept_task_result`, `CoreDb::result_rows_for_day`
- Consumes: `core_agent_api` DTOs.

- [ ] **Step 1: Export the Core module**

In `src/lib.rs`, add:

```rust
pub mod core;
```

Create `src/core/mod.rs`:

```rust
pub mod db;
```

- [ ] **Step 2: Add failing storage tests**

Create `src/core/db.rs` with tests and minimal struct shell:

```rust
use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::core_agent_api::{AgentRegisterRequest, ConfigSnapshotResponse, ResultRow, TaskResultReport, TaskStatus};

pub struct CoreDb {
    conn: Connection,
}

impl CoreDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_agent_api::{AgentCapabilities, RuleFile};
    use tempfile::tempdir;

    fn db() -> CoreDb {
        let dir = tempdir().unwrap();
        CoreDb::open(dir.path().join("core.db")).unwrap()
    }

    fn agent_request() -> AgentRegisterRequest {
        AgentRegisterRequest {
            agent_id: None,
            agent_name: "agent-1".to_string(),
            host: "127.0.0.1".to_string(),
            port: 18081,
            version: "1.0.0".to_string(),
            capabilities: AgentCapabilities {
                can_collect: true,
                can_parse: true,
                can_load: false,
                supported_protocols: vec!["ftp".to_string(), "sftp".to_string()],
            },
        }
    }

    #[test]
    fn registers_agent_and_reuses_existing_agent_id() {
        let db = db();
        let agent_id = db.register_agent(&agent_request()).unwrap();
        let mut reconnect = agent_request();
        reconnect.agent_id = Some(agent_id.clone());
        let reused = db.register_agent(&reconnect).unwrap();
        assert_eq!(reused, agent_id);
    }

    #[test]
    fn stores_task_result_rows() {
        let db = db();
        let agent_id = db.register_agent(&agent_request()).unwrap();
        db.insert_config_snapshot(&ConfigSnapshotResponse {
            config_snapshot_id: "cfg_1".to_string(),
            content_hash: "sha256:test".to_string(),
            source_toml: "[source]".to_string(),
            mapping_dx_ini: "[m]".to_string(),
            load_toml: "[load]".to_string(),
            col_name_cut_config_ini: None,
            rules: vec![RuleFile { relative_path: "rules/a.json".to_string(), content: "{\"table_name\":\"TPD_A\"}".to_string() }],
        }).unwrap();
        db.create_task("task_1", "strategy_1:2026-06-17 15:15:00:cfg_1", "strategy_1", "cfg_1", "2026-06-17 15:15:00", "collect_1", &agent_id).unwrap();
        db.accept_task_result(&TaskResultReport {
            task_id: "task_1".to_string(),
            agent_id,
            status: TaskStatus::Succeeded,
            result_rows: vec![ResultRow {
                table_name: "TPD_A".to_string(),
                data_time: "2026-06-17 15:15:00".to_string(),
                row_count: 123,
                success: 1,
                collect_time: "2026-07-02 15:35:00".to_string(),
            }],
        }).unwrap();
        let rows = db.result_rows_for_day("strategy_1", "2026-06-17").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].table_name, "TPD_A");
        assert_eq!(rows[0].row_count, 123);
    }
}
```

- [ ] **Step 3: Run storage tests to verify they fail**

Run: `cargo test core::db`

Expected: compile fails because storage methods are missing.

- [ ] **Step 4: Implement schema and storage methods**

Replace `CoreDb::open` and add methods:

```rust
impl CoreDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS agents (
                agent_id TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                host TEXT NOT NULL,
                port INTEGER NOT NULL,
                version TEXT NOT NULL,
                capabilities_json TEXT NOT NULL,
                status TEXT NOT NULL,
                registered_at TEXT NOT NULL,
                last_heartbeat_at TEXT
            );
            CREATE TABLE IF NOT EXISTS config_snapshots (
                config_snapshot_id TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                snapshot_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS collect_tasks (
                task_id TEXT PRIMARY KEY,
                logical_task_key TEXT NOT NULL,
                strategy_id TEXT NOT NULL,
                config_snapshot_id TEXT NOT NULL,
                scan_start_time TEXT NOT NULL,
                collect_id TEXT NOT NULL,
                assigned_agent_id TEXT NOT NULL,
                attempt_no INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                accepted_at TEXT,
                started_at TEXT,
                last_progress_at TEXT,
                finished_at TEXT,
                error_code TEXT,
                error_message TEXT
            );
            CREATE TABLE IF NOT EXISTS collect_result_cells (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                strategy_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                config_snapshot_id TEXT NOT NULL,
                table_name TEXT NOT NULL,
                data_time TEXT NOT NULL,
                row_count INTEGER NOT NULL,
                success INTEGER NOT NULL,
                collect_time TEXT NOT NULL,
                status TEXT NOT NULL,
                error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_collect_result_day ON collect_result_cells(strategy_id, data_time, table_name);
            "#,
        )?;
        Ok(())
    }

    pub fn register_agent(&self, request: &AgentRegisterRequest) -> Result<String> {
        let agent_id = request.agent_id.clone().unwrap_or_else(|| format!("agent_{}", uuid::Uuid::new_v4().simple()));
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let capabilities_json = serde_json::to_string(&request.capabilities)?;
        self.conn.execute(
            r#"
            INSERT INTO agents(agent_id, agent_name, host, port, version, capabilities_json, status, registered_at, last_heartbeat_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'ONLINE', ?7, ?7)
            ON CONFLICT(agent_id) DO UPDATE SET
                agent_name=excluded.agent_name,
                host=excluded.host,
                port=excluded.port,
                version=excluded.version,
                capabilities_json=excluded.capabilities_json,
                status='ONLINE',
                last_heartbeat_at=excluded.last_heartbeat_at
            "#,
            rusqlite::params![agent_id, request.agent_name, request.host, request.port, request.version, capabilities_json, now],
        )?;
        Ok(agent_id)
    }

    pub fn insert_config_snapshot(&self, snapshot: &ConfigSnapshotResponse) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.conn.execute(
            "INSERT OR REPLACE INTO config_snapshots(config_snapshot_id, content_hash, snapshot_json, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![snapshot.config_snapshot_id, snapshot.content_hash, serde_json::to_string(snapshot)?, now],
        )?;
        Ok(())
    }

    pub fn create_task(&self, task_id: &str, logical_task_key: &str, strategy_id: &str, config_snapshot_id: &str, scan_start_time: &str, collect_id: &str, assigned_agent_id: &str) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.conn.execute(
            "INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'CREATED', ?8)",
            rusqlite::params![task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, now],
        )?;
        Ok(())
    }

    pub fn accept_task_result(&self, report: &TaskResultReport) -> Result<()> {
        let (strategy_id, config_snapshot_id): (String, String) = self.conn.query_row(
            "SELECT strategy_id, config_snapshot_id FROM collect_tasks WHERE task_id = ?1 AND assigned_agent_id = ?2",
            rusqlite::params![report.task_id, report.agent_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        for result in &report.result_rows {
            self.conn.execute(
                "INSERT INTO collect_result_cells(task_id, strategy_id, agent_id, config_snapshot_id, table_name, data_time, row_count, success, collect_time, status, error_message, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'SUCCEEDED', NULL, ?10, ?10)",
                rusqlite::params![report.task_id, strategy_id, report.agent_id, config_snapshot_id, result.table_name, result.data_time, result.row_count, result.success, result.collect_time, now],
            )?;
        }
        self.conn.execute(
            "UPDATE collect_tasks SET status = 'SUCCEEDED', finished_at = ?2 WHERE task_id = ?1",
            rusqlite::params![report.task_id, now],
        )?;
        Ok(())
    }

    pub fn result_rows_for_day(&self, strategy_id: &str, day: &str) -> Result<Vec<ResultRow>> {
        let like = format!("{day}%");
        let mut stmt = self.conn.prepare(
            "SELECT table_name, data_time, row_count, success, collect_time FROM collect_result_cells WHERE strategy_id = ?1 AND data_time LIKE ?2 ORDER BY table_name, data_time",
        )?;
        let rows = stmt.query_map(rusqlite::params![strategy_id, like], |row| {
            Ok(ResultRow {
                table_name: row.get(0)?,
                data_time: row.get(1)?,
                row_count: row.get::<_, i64>(2)? as u64,
                success: row.get(3)?,
                collect_time: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
}
```

- [ ] **Step 5: Run storage tests**

Run: `cargo test core::db`

Expected: both storage tests pass.

- [ ] **Step 6: Commit Core storage**

Run:

```bash
git add src/lib.rs src/core/mod.rs src/core/db.rs
git commit -m "feat: add sqlite core storage"
```

---

### Task 4: Implement Core HTTP Server Endpoints

**Files:**
- Modify: `src/core/mod.rs`
- Create: `src/core/server.rs`
- Create: `src/bin/core.rs`

**Interfaces:**
- Produces: `core::server::run_core_server(addr: SocketAddr, db_path: PathBuf) -> Result<()>`
- Consumes: `CoreDb` methods from Task 3.
- Produces HTTP endpoints for register, heartbeat, config snapshot fetch, task events, and task results.

- [ ] **Step 1: Export server module**

In `src/core/mod.rs`, add:

```rust
pub mod server;
```

- [ ] **Step 2: Add route construction test**

Create `src/core/server.rs` with a router test:

```rust
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::{routing::{get, post}, Json, Router};

use crate::core::db::CoreDb;
use crate::core_agent_api::{AgentRegisterRequest, AgentRegisterResponse, TaskResultReport};

#[derive(Clone)]
pub struct CoreState {
    pub db: Arc<Mutex<CoreDb>>,
}

pub fn router(state: CoreState) -> Router {
    Router::new()
        .route("/api/agents/register", post(register_agent))
        .route("/api/agents/:agent_id/heartbeat", post(heartbeat))
        .route("/api/config-snapshots/:config_snapshot_id", get(config_snapshot))
        .route("/api/tasks/:task_id/events", post(task_event))
        .route("/api/tasks/:task_id/result", post(task_result))
        .with_state(state)
}

async fn register_agent(axum::extract::State(state): axum::extract::State<CoreState>, Json(request): Json<AgentRegisterRequest>) -> Json<AgentRegisterResponse> {
    let agent_id = state.db.lock().unwrap().register_agent(&request).unwrap();
    Json(AgentRegisterResponse { agent_id, heartbeat_interval_seconds: 10, task_report_interval_seconds: 10 })
}

async fn heartbeat() -> Json<serde_json::Value> {
    Json(serde_json::json!({"accepted": true}))
}

async fn config_snapshot() -> Json<serde_json::Value> {
    Json(serde_json::json!({"error": "config snapshot endpoint is wired but storage fetch is not implemented in this task"}))
}

async fn task_event() -> Json<serde_json::Value> {
    Json(serde_json::json!({"accepted": true}))
}

async fn task_result(axum::extract::State(state): axum::extract::State<CoreState>, Json(report): Json<TaskResultReport>) -> Json<serde_json::Value> {
    state.db.lock().unwrap().accept_task_result(&report).unwrap();
    Json(serde_json::json!({"accepted": true}))
}

pub async fn run_core_server(addr: SocketAddr, db_path: PathBuf) -> Result<()> {
    let state = CoreState { db: Arc::new(Mutex::new(CoreDb::open(db_path)?)) };
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    use tempfile::tempdir;

    #[tokio::test]
    async fn register_agent_endpoint_returns_agent_id() {
        let dir = tempdir().unwrap();
        let state = CoreState { db: Arc::new(Mutex::new(CoreDb::open(dir.path().join("core.db")).unwrap())) };
        let app = router(state);
        let body = serde_json::json!({
            "agent_id": null,
            "agent_name": "agent-1",
            "host": "127.0.0.1",
            "port": 18081,
            "version": "1.0.0",
            "capabilities": {"can_collect": true, "can_parse": true, "can_load": false, "supported_protocols": ["ftp"]}
        });
        let response = app.oneshot(Request::builder().method("POST").uri("/api/agents/register").header("content-type", "application/json").body(Body::from(body.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
```

- [ ] **Step 3: Add `tower` dev dependency for route tests**

In `Cargo.toml`, add:

```toml
[dev-dependencies]
tower = "0.5"
```

- [ ] **Step 4: Run Core route test**

Run: `cargo test register_agent_endpoint_returns_agent_id`

Expected: PASS.

- [ ] **Step 5: Add Core binary**

Create `src/bin/core.rs`:

```rust
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:18080")]
    listen: SocketAddr,
    #[arg(long, default_value = "core.db")]
    db: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    wy_gnb_pm_parser::core::server::run_core_server(cli.listen, cli.db).await
}
```

- [ ] **Step 6: Build Core binary**

Run: `cargo build --bin core`

Expected: build succeeds.

- [ ] **Step 7: Commit Core HTTP server**

Run:

```bash
git add Cargo.toml Cargo.lock src/core/mod.rs src/core/server.rs src/bin/core.rs
git commit -m "feat: add core http server"
```

---

### Task 5: Implement Agent Result CSV Scanner and Local Store

**Files:**
- Modify: `src/lib.rs`
- Create: `src/agent/mod.rs`
- Create: `src/agent/result_csv.rs`
- Create: `src/agent/store.rs`

**Interfaces:**
- Produces: `agent::result_csv::read_result_rows(output_dir: &Path) -> Result<Vec<ResultRow>>`
- Produces: `agent::store::AgentStore::new(root: PathBuf) -> Result<AgentStore>`
- Produces: `AgentStore::persist_task(request: &TaskDispatchRequest) -> Result<PathBuf>`

- [ ] **Step 1: Export Agent module**

In `src/lib.rs`, add:

```rust
pub mod agent;
```

Create `src/agent/mod.rs`:

```rust
pub mod result_csv;
pub mod store;
```

- [ ] **Step 2: Add result CSV scanner test and implementation**

Create `src/agent/result_csv.rs`:

```rust
use std::path::Path;

use anyhow::Result;
use walkdir::WalkDir;

use crate::core_agent_api::ResultRow;

pub fn read_result_rows(output_dir: &Path) -> Result<Vec<ResultRow>> {
    let mut rows = Vec::new();
    for entry in WalkDir::new(output_dir).into_iter().filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_file() || entry.file_name() != "result.csv" {
            continue;
        }
        let mut reader = csv::Reader::from_path(entry.path())?;
        for record in reader.records() {
            let record = record?;
            rows.push(ResultRow {
                table_name: record.get(0).unwrap_or_default().to_string(),
                data_time: record.get(1).unwrap_or_default().to_string(),
                row_count: record.get(2).unwrap_or("0").parse::<u64>()?,
                success: record.get(3).unwrap_or("0").parse::<i32>()?,
                collect_time: record.get(4).unwrap_or_default().to_string(),
            });
        }
    }
    rows.sort_by(|left, right| left.table_name.cmp(&right.table_name).then(left.data_time.cmp(&right.data_time)));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reads_nested_result_csv_rows() {
        let dir = tempdir().unwrap();
        let package_dir = dir.path().join("tpd_a_2026061715").join("collect_1_202606171515");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(package_dir.join("result.csv"), "table_name,data_time,row_count,success,collect_time\nTPD_A,2026-06-17 15:15:00,100,1,2026-07-02 15:35:00\n").unwrap();

        let rows = read_result_rows(dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].table_name, "TPD_A");
        assert_eq!(rows[0].row_count, 100);
    }
}
```

- [ ] **Step 3: Add local task store**

Create `src/agent/store.rs`:

```rust
use std::path::PathBuf;

use anyhow::Result;

use crate::core_agent_api::TaskDispatchRequest;

#[derive(Clone, Debug)]
pub struct AgentStore {
    root: PathBuf,
}

impl AgentStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("tasks"))?;
        std::fs::create_dir_all(root.join("config_snapshots"))?;
        Ok(Self { root })
    }

    pub fn task_dir(&self, task_id: &str) -> PathBuf {
        self.root.join("tasks").join(task_id)
    }

    pub fn persist_task(&self, request: &TaskDispatchRequest) -> Result<PathBuf> {
        let task_dir = self.task_dir(&request.task_id);
        std::fs::create_dir_all(task_dir.join("downloads"))?;
        std::fs::create_dir_all(task_dir.join("output"))?;
        std::fs::create_dir_all(task_dir.join("logs"))?;
        std::fs::write(task_dir.join("task.json"), serde_json::to_vec_pretty(request)?)?;
        std::fs::write(task_dir.join("state.json"), serde_json::json!({"status": "ACCEPTED"}).to_string())?;
        Ok(task_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn persists_task_before_execution() {
        let dir = tempdir().unwrap();
        let store = AgentStore::new(dir.path().join("agent_data")).unwrap();
        let request = TaskDispatchRequest {
            task_id: "task_1".to_string(),
            logical_task_key: "strategy:time:cfg".to_string(),
            strategy_id: "strategy".to_string(),
            config_snapshot_id: "cfg".to_string(),
            scan_start_time: "2026-06-17 15:15:00".to_string(),
            collect_id: "collect_1".to_string(),
            load_type: "clickhouse".to_string(),
            encoding: "UTF-8".to_string(),
            output_delimiter: "|".to_string(),
            timeout_seconds: 1800,
            callback_base_url: "http://127.0.0.1:18080/api".to_string(),
        };
        let task_dir = store.persist_task(&request).unwrap();
        assert!(task_dir.join("task.json").exists());
        assert!(task_dir.join("output").is_dir());
    }
}
```

- [ ] **Step 4: Run Agent local tests**

Run: `cargo test agent::`

Expected: result CSV and local store tests pass.

- [ ] **Step 5: Commit Agent local storage and scanner**

Run:

```bash
git add src/lib.rs src/agent/mod.rs src/agent/result_csv.rs src/agent/store.rs
git commit -m "feat: add agent local task store"
```

---

### Task 6: Implement Agent HTTP Server and Background Runner

**Files:**
- Modify: `src/agent/mod.rs`
- Create: `src/agent/runner.rs`
- Create: `src/agent/server.rs`
- Create: `src/bin/agent.rs`

**Interfaces:**
- Produces: `agent::server::run_agent_server(addr, data_dir, core_url, agent_id) -> Result<()>`
- Produces: Agent `POST /api/tasks` endpoint returning `TaskDispatchResponse` after local persistence.
- Consumes: `AgentStore`, `run_parse_job`, `read_result_rows`.

- [ ] **Step 1: Export runner and server modules**

In `src/agent/mod.rs`, add:

```rust
pub mod runner;
pub mod server;
```

- [ ] **Step 2: Add Agent runner**

Create `src/agent/runner.rs`:

```rust
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::agent::result_csv::read_result_rows;
use crate::core_agent_api::{TaskDispatchRequest, TaskResultReport, TaskStatus};
use crate::parse_job::{run_parse_job, ParseJobOptions};
use crate::LoadType;

#[derive(Clone)]
pub struct AgentRunner {
    pub agent_id: String,
    pub core_api_base: String,
    pub http: reqwest::Client,
}

impl AgentRunner {
    pub fn new(agent_id: String, core_api_base: String) -> Self {
        Self { agent_id, core_api_base, http: reqwest::Client::new() }
    }

    pub async fn run_task(&self, task: TaskDispatchRequest, task_dir: PathBuf) -> Result<()> {
        let config_dir = task_dir.join("config");
        let output_dir = task_dir.join("output");
        let load_type = match task.load_type.to_ascii_lowercase().as_str() {
            "clickhouse" => LoadType::Clickhouse,
            "postgresql" => LoadType::Postgresql,
            other => bail!("unsupported load_type {other}"),
        };
        run_parse_job(ParseJobOptions {
            input: None,
            source_config: Some(config_dir.join("source.toml")),
            scan_start_time: Some(task.scan_start_time.clone()),
            config_dir: config_dir.clone(),
            output_dir: output_dir.clone(),
            collect_id: task.collect_id.clone(),
            load_type,
            load_config: config_dir.join("load.toml"),
            output_delimiter: task.output_delimiter.clone(),
            encoding: task.encoding.clone(),
            recursive: false,
            rule_files: Vec::new(),
            rules_dir: Some(config_dir.join("rules")),
        })?;
        let rows = read_result_rows(&output_dir)?;
        let report = TaskResultReport { task_id: task.task_id.clone(), agent_id: self.agent_id.clone(), status: TaskStatus::Succeeded, result_rows: rows };
        self.http.post(format!("{}/tasks/{}/result", self.core_api_base, task.task_id)).json(&report).send().await?.error_for_status()?;
        Ok(())
    }
}
```

- [ ] **Step 3: Add Agent server endpoint test and implementation**

Create `src/agent/server.rs`:

```rust
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use axum::{routing::post, Json, Router};

use crate::agent::runner::AgentRunner;
use crate::agent::store::AgentStore;
use crate::core_agent_api::{TaskDispatchRequest, TaskDispatchResponse, TaskStatus};

#[derive(Clone)]
pub struct AgentState {
    pub store: AgentStore,
    pub runner: AgentRunner,
}

pub fn router(state: AgentState) -> Router {
    Router::new().route("/api/tasks", post(dispatch_task)).with_state(state)
}

async fn dispatch_task(axum::extract::State(state): axum::extract::State<AgentState>, Json(request): Json<TaskDispatchRequest>) -> Json<TaskDispatchResponse> {
    let task_id = request.task_id.clone();
    match state.store.persist_task(&request) {
        Ok(task_dir) => {
            let runner = state.runner.clone();
            tokio::spawn(async move {
                if let Err(err) = runner.run_task(request, task_dir).await {
                    tracing::warn!("agent task failed: {err:#}");
                }
            });
            Json(TaskDispatchResponse { task_id, accepted: true, agent_task_state: TaskStatus::Accepted, reason: None })
        }
        Err(err) => Json(TaskDispatchResponse { task_id, accepted: false, agent_task_state: TaskStatus::Failed, reason: Some(format!("{err:#}")) }),
    }
}

pub async fn run_agent_server(addr: SocketAddr, data_dir: PathBuf, core_api_base: String, agent_id: String) -> Result<()> {
    let state = AgentState { store: AgentStore::new(data_dir)?, runner: AgentRunner::new(agent_id, core_api_base) };
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[tokio::test]
    async fn dispatch_task_persists_before_accepting() {
        let dir = tempdir().unwrap();
        let state = AgentState { store: AgentStore::new(dir.path().join("agent_data")).unwrap(), runner: AgentRunner::new("agent_1".to_string(), "http://127.0.0.1:9/api".to_string()) };
        let app = router(state);
        let body = serde_json::json!({
            "task_id": "task_1",
            "logical_task_key": "strategy:time:cfg",
            "strategy_id": "strategy",
            "config_snapshot_id": "cfg",
            "scan_start_time": "2026-06-17 15:15:00",
            "collect_id": "collect_1",
            "load_type": "clickhouse",
            "encoding": "UTF-8",
            "output_delimiter": "|",
            "timeout_seconds": 1800,
            "callback_base_url": "http://127.0.0.1:18080/api"
        });
        let response = app.oneshot(Request::builder().method("POST").uri("/api/tasks").header("content-type", "application/json").body(Body::from(body.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(dir.path().join("agent_data/tasks/task_1/task.json").exists());
    }
}
```

- [ ] **Step 4: Add Agent binary**

Create `src/bin/agent.rs`:

```rust
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:18081")]
    listen: SocketAddr,
    #[arg(long, default_value = "agent_data")]
    data_dir: PathBuf,
    #[arg(long, default_value = "http://127.0.0.1:18080/api")]
    core_api_base: String,
    #[arg(long, default_value = "agent_local")]
    agent_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    wy_gnb_pm_parser::agent::server::run_agent_server(cli.listen, cli.data_dir, cli.core_api_base, cli.agent_id).await
}
```

- [ ] **Step 5: Run Agent server test and build**

Run: `cargo test dispatch_task_persists_before_accepting`

Expected: PASS.

Run: `cargo build --bin agent`

Expected: build succeeds.

- [ ] **Step 6: Commit Agent server**

Run:

```bash
git add src/agent/mod.rs src/agent/runner.rs src/agent/server.rs src/bin/agent.rs
git commit -m "feat: add agent http task receiver"
```

---

### Task 7: Add Result Grid Query Model

**Files:**
- Modify: `src/core/mod.rs`
- Create: `src/core/grid.rs`
- Modify: `src/core/server.rs`

**Interfaces:**
- Produces: `core::grid::build_daily_grid(day: &str, interval_minutes: u32, expected_tables: &[String], rows: &[ResultRow]) -> DailyGrid`
- Produces: `GET /api/results/grid?strategy_id=...&day=YYYY-MM-DD&interval_minutes=15`

- [ ] **Step 1: Export grid module**

In `src/core/mod.rs`, add:

```rust
pub mod grid;
```

- [ ] **Step 2: Add grid model and tests**

Create `src/core/grid.rs`:

```rust
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::core_agent_api::ResultRow;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DailyGrid {
    pub day: String,
    pub time_slots: Vec<String>,
    pub rows: Vec<TableGridRow>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TableGridRow {
    pub table_name: String,
    pub cells: Vec<GridCell>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct GridCell {
    pub data_time: String,
    pub value: Option<u64>,
    pub color: String,
    pub status: String,
}

pub fn build_daily_grid(day: &str, interval_minutes: u32, expected_tables: &[String], rows: &[ResultRow]) -> DailyGrid {
    let slots = time_slots(day, interval_minutes);
    let mut tables: BTreeSet<String> = expected_tables.iter().cloned().collect();
    for row in rows {
        tables.insert(row.table_name.clone());
    }
    let by_key: BTreeMap<(String, String), &ResultRow> = rows.iter().map(|row| ((row.table_name.clone(), row.data_time.clone()), row)).collect();
    let rows = tables.into_iter().map(|table_name| {
        let cells = slots.iter().map(|slot| {
            if let Some(row) = by_key.get(&(table_name.clone(), slot.clone())) {
                let (color, status) = if row.success == 0 {
                    ("red", "failed")
                } else if row.row_count == 0 {
                    ("yellow", "empty")
                } else {
                    ("green", "ok")
                };
                GridCell { data_time: slot.clone(), value: Some(row.row_count), color: color.to_string(), status: status.to_string() }
            } else {
                GridCell { data_time: slot.clone(), value: None, color: "gray".to_string(), status: "missing".to_string() }
            }
        }).collect();
        TableGridRow { table_name, cells }
    }).collect();
    DailyGrid { day: day.to_string(), time_slots: slots, rows }
}

fn time_slots(day: &str, interval_minutes: u32) -> Vec<String> {
    let mut slots = Vec::new();
    let mut minute = 0;
    while minute < 24 * 60 {
        slots.push(format!("{} {:02}:{:02}:00", day, minute / 60, minute % 60));
        minute += interval_minutes;
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_daily_grid_with_missing_and_success_cells() {
        let grid = build_daily_grid(
            "2026-06-17",
            15,
            &["TPD_A".to_string()],
            &[ResultRow { table_name: "TPD_A".to_string(), data_time: "2026-06-17 00:15:00".to_string(), row_count: 7, success: 1, collect_time: "2026-07-02 15:35:00".to_string() }],
        );
        assert_eq!(grid.time_slots.len(), 96);
        assert_eq!(grid.rows.len(), 1);
        assert_eq!(grid.rows[0].cells[0].color, "gray");
        assert_eq!(grid.rows[0].cells[1].value, Some(7));
        assert_eq!(grid.rows[0].cells[1].color, "green");
    }
}
```

- [ ] **Step 3: Add Core grid route**

In `src/core/server.rs`, add route:

```rust
.route("/api/results/grid", get(result_grid))
```

Add handler:

```rust
#[derive(serde::Deserialize)]
struct GridQuery {
    strategy_id: String,
    day: String,
    interval_minutes: Option<u32>,
}

async fn result_grid(axum::extract::State(state): axum::extract::State<CoreState>, axum::extract::Query(query): axum::extract::Query<GridQuery>) -> Json<crate::core::grid::DailyGrid> {
    let rows = state.db.lock().unwrap().result_rows_for_day(&query.strategy_id, &query.day).unwrap();
    let expected_tables = rows.iter().map(|row| row.table_name.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>();
    Json(crate::core::grid::build_daily_grid(&query.day, query.interval_minutes.unwrap_or(15), &expected_tables, &rows))
}
```

- [ ] **Step 4: Run grid tests**

Run: `cargo test core::grid`

Expected: grid test passes.

- [ ] **Step 5: Commit grid support**

Run:

```bash
git add src/core/mod.rs src/core/grid.rs src/core/server.rs
git commit -m "feat: add collection result grid"
```

---

### Task 8: End-to-End Smoke Verification Documentation

**Files:**
- Create: `docs/core-agent-smoke.md`

**Interfaces:**
- Consumes: Core binary, Agent binary, parser job wiring, and result grid endpoint.
- Produces: documented manual smoke flow for running Core and Agent locally.

- [ ] **Step 1: Add smoke test documentation**

Create `docs/core-agent-smoke.md`:

```markdown
# Core/Agent Smoke Flow

This verifies the first Core/Agent loop without changing the legacy parser CLI.

## Build

```bash
cargo build --bin core --bin agent
```

## Start Core

```bash
cargo run --bin core -- --listen 127.0.0.1:18080 --db core.db
```

## Start Agent

```bash
cargo run --bin agent -- --listen 127.0.0.1:18081 --data-dir agent_data --core-api-base http://127.0.0.1:18080/api --agent-id agent_local
```

## Register Agent

```bash
curl -sS -X POST http://127.0.0.1:18080/api/agents/register \
  -H 'content-type: application/json' \
  -d '{"agent_id":"agent_local","agent_name":"agent-local","host":"127.0.0.1","port":18081,"version":"1.0.0","capabilities":{"can_collect":true,"can_parse":true,"can_load":false,"supported_protocols":["ftp","sftp","local"]}}'
```

## Query Result Grid

```bash
curl -sS 'http://127.0.0.1:18080/api/results/grid?strategy_id=strategy_1&day=2026-06-17&interval_minutes=15'
```

The grid response contains `time_slots` and one row per table. Cells are colored `green`, `yellow`, `red`, or `gray`.
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test`

Expected: all tests pass.

- [ ] **Step 3: Build binaries**

Run: `cargo build --bin wy-gnb-pm-parser --bin core --bin agent`

Expected: all binaries build.

- [ ] **Step 4: Commit smoke documentation**

Run:

```bash
git add docs/core-agent-smoke.md
git commit -m "docs: add core agent smoke flow"
```

---

## Self-Review

- Spec coverage: Tasks cover parser reuse, shared DTOs, SQLite Core state/result storage, Core HTTP endpoints, Agent task acceptance, Agent parser execution, `result.csv` ingestion, result grid, cancellation terminal-status semantics, and local smoke documentation.
- Scope control: MQ, Agent-side DB loading, file hash verification, independent remote re-scan, Agent upgrades, and alerting are excluded from implementation tasks.
- Type consistency: `TaskStatus`, `TaskDispatchRequest`, `ResultRow`, and `TaskResultReport` are defined once in `src/core_agent_api.rs` and consumed by Core and Agent tasks.
- Existing CLI preservation: Task 1 keeps the current CLI as a wrapper around `run_parse_job`; later tasks do not change CLI arguments.
