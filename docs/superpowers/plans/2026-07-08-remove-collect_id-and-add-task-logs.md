# Remove collect_id and Add Task Logs

> **For agentic workers:** Implementation plan.

**Goal:** Remove `collect_id` from `collect_tasks` table, replace usage with `collector_name`/`task_id`, and add per-task log output to `logs/` directory.

**Architecture:** Three independent changes: (1) tracing-appender per-task logs in agent runner, (2) remove `collect_id` from DB schema and all Rust structs, replace with `task_id` for output paths and `collector_name` for DB persistence, (3) update frontend.

**Tech Stack:** Rust, tracing/tracing-appender, React/Ant Design

---

### Task 1: Add per-task log output in agent runner

**Files:**
- Modify: `src/agent/runner.rs`

tracing-appender is already in Cargo.toml. In `run_parse_and_report`, create a non-blocking file writer to `task_dir/logs/agent.log` and layer it on top of the existing console logging.

```rust
// In run_parse_and_report, after getting task_dir:
let log_file = std::fs::File::create(task_dir.join("logs").join("agent.log"))?;
let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);

let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| "debug".parse().unwrap());

let subscriber = tracing_subscriber::registry()
    .with(env_filter)
    .with(
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(true)
    )
    .with(
        tracing_subscriber::fmt::layer()
            .with_writer(non_blocking)
            .with_ansi(false)
    );

let _guard = tracing::subscriber::set_default(subscriber);
```

### Task 2: Remove collect_id from Rust structs and DB

**Files:**
- Modify: `src/core_agent_api.rs` (remove from `TaskDispatchRequest`)
- Modify: `src/agent/runner.rs` (use `task.task_id` instead of `task.collect_id`)
- Modify: `src/parse_job.rs` (remove `collect_id` from `ParseJobOptions`)
- Modify: `src/writer.rs` (replace `collect_id` with `task_id` in `WriteOptions`)
- Modify: `src/tpd.rs` (replace `collect_id` with `task_id` in `StreamingFinishOptions`)
- Modify: `src/core/db.rs` (remove `collect_id` from CREATE TABLE, create_task, INSERT, SELECT, implicit INSERT)
- Modify: `src/core/server.rs` (remove `collect_id` generation and dispatch)
- Modify: `src/main.rs` (remove `--collect-id` from CLI)
- Modify: `src/agent/store.rs` (test fixture)

### Task 3: Update frontend

**Files:**
- Modify: `pm-admin/src/types/api.ts` (remove `collect_id` from `TaskDispatchRequest`)
- Modify: `pm-admin/src/pages/Tasks/index.tsx` (remove `collect_id` from form and type)

### Task 4: Build and test

```bash
cargo test && cargo build --release && cp target/release/core test/core && cp target/release/agent test/agent
```
