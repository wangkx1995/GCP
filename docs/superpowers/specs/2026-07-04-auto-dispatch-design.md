# Auto-Dispatch: Immediate Strategy Task Dispatch with Inline Connection Params

## Scope

Move source/load connection configuration from config-snapshot files (`source.toml`, `load.toml`) into the `data_collector_unit` configuration, auto-dispatch tasks when creating immediate collection strategies, and pass all connection params inline via `TaskDispatchRequest` so agents do not read local config files.

## Data Model

### data_collector_unit — New Columns

| Column | Type | Default | Purpose |
|--------|------|---------|---------|
| `load_type` | TEXT | `clickhouse` | DB type: `clickhouse` / `postgresql` |
| `output_delimiter` | TEXT | `\|` | CSV output delimiter |
| `db_host` | TEXT | `""` | DB server host |
| `db_port` | INTEGER | `9000` | DB server port |
| `db_user` | TEXT | `""` | DB login user |
| `db_password` | TEXT | `""` | DB login password |
| `db_database` | TEXT | `""` | DB database name |
| `db_table_name_case` | TEXT | `lower` | `lower` / `upper` (ClickHouse) |

Adding these columns via SQLite ALTER TABLE migration.

### DataCollectorUnitRow / DataCollectorUnitSaveRequest

Add corresponding fields to both Rust structs. The save request fields are `Option<T>` where applicable so the frontend can send partial updates.

## TaskDispatchRequest — New Fields

Add to the existing struct:

| Field | Type | Source |
|-------|------|--------|
| `source_type` | String | unit.source_type |
| `remote_pattern` | String | unit.remote_pattern |
| `host` | String | unit.host |
| `port` | u16 | unit.port |
| `username` | String | unit.username |
| `password` | String | unit.password |
| `connect_retry` | i64 | unit.connect_retry |
| `download_retry` | i64 | unit.download_retry |
| `download_parallel` | i64 | unit.download_parallel |
| `retry_interval_secs` | i64 | unit.retry_interval_secs |
| `connect_timeout_secs` | i64 | unit.connect_timeout_secs |
| `read_timeout_secs` | i64 | unit.read_timeout_secs |
| `cache_retention_days` | i64 | unit.cache_retention_days |
| `db_host` | String | unit.db_host |
| `db_port` | u16 | unit.db_port |
| `db_user` | String | unit.db_user |
| `db_password` | String | unit.db_password |
| `db_database` | String | unit.db_database |
| `db_table_name_case` | String | unit.db_table_name_case |

Existing fields `load_type`, `output_delimiter`, `encoding`, `timeout_seconds` remain unchanged.

## Auto-Dispatch Flow

### Backend: create_strategies Handler

1. Insert N strategy rows (one per table_name) — existing logic
2. For each inserted row:
   a. Query `data_collector_unit` by `collector_id` — get source + DB connection params
   b. Query active `config_snapshot` by unit's `config_name` — get config_snapshot_id
   c. Build `TaskDispatchRequest`:
      - `task_id` = `task_immediate_{strategy_id}_{timestamp}`
      - `logical_task_key` = `strategy_{strategy_id}:{scan_start_time}`
      - `strategy_id` = strategy row's id
      - `config_snapshot_id` = from step b
      - `scan_start_time` = strategy's execute_time (or now if null)
      - `collect_id` = `collect_immediate_{strategy_id}_{timestamp}`
      - Source/DB params from step a
   d. Select online agent (existing `select_online_agent`)
   e. Create task record (existing `create_task`)
   f. Forward to agent via HTTP POST (existing `dispatch_task` logic)
3. Return strategies list + per-row dispatch status

If agent is unavailable for a particular row, that strategy row is still saved (status remains `可用`); the response includes per-row error info.

### Frontend

No dispatch-related frontend changes — the POST `/api/strategies` call remains the same. The form already collects `load_type`, `output_delimiter`, and DB connection fields as part of the data_collector_unit form.

## Agent Changes

### AgentRunner::run_task

Before running `run_parse_job`, construct config structs from request params instead of reading files:

1. **SourceConfig** — construct `remote_file_source::SourceConfig` from `TaskDispatchRequest` fields. The `download_dir` is set to the task's local download path (agent-managed, not from request).

2. **LoadConfig** — construct `LoadConfig` from request params. The `client` field is derived from `load_type` (`clickhouse` → `clickhouse-client`, `postgresql` → `psql`).

### ParseJobOptions Changes

| Field | Before | After |
|-------|--------|-------|
| `source_config` | `Option<PathBuf>` | `Option<SourceConfig>` (parsed struct, not path) |
| `load_config` | `PathBuf` | `LoadConfig` (parsed struct, not path) |

### remote_file_source::ResolveOptions Changes

| Field | Before | After |
|-------|--------|-------|
| `source_config` | `Option<PathBuf>` | `Option<SourceConfig>` |

The `remote_file_source` crate's `SourceConfig`, `SourceSection`, `ConnectionConfig`, `SourceKind` types are made `pub` so the agent can construct them directly. The file-reading path (`load_source_config`) is kept for backward compatibility and tests.

## Config Snapshot Impact

`source.toml` and `load.toml` remain in the config snapshot zip (required for validation), but agents will use the params from `TaskDispatchRequest` instead. The files are still required in the snapshot for: (a) legacy CLI usage, (b) test fixtures, (c) the `use_remote` check in runner.rs which detects source.toml existence.

## Implementation Order

1. DB migration — add columns to `data_collector_unit`
2. Backend types — update `DataCollectorUnitRow`, `DataCollectorUnitSaveRequest`, `TaskDispatchRequest`
3. Backend auto-dispatch — modify `create_strategies` handler
4. Frontend — add fields to data_collector_unit form
5. remote_file_source — make types public, change `ResolveOptions`
6. Agent — construct configs from params in `run_task`
7. Tests — update existing, add integration test for dispatch flow
