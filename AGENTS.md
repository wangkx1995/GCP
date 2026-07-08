# AGENTS.md - wy-gnb-pm-parser

## What This Is

Rust CLI (`.csv`/`.gz`/`.zip` PM parsing) + Axum HTTP core + agent daemon.
Three binaries auto-detected from `src/main.rs`, `src/bin/core.rs`, `src/bin/agent.rs`.

## Workspace Layout

```
src/
├── main.rs              # standalone parser CLI (clap)
├── bin/core.rs          # Axum HTTP server + TCP agent registry
├── bin/agent.rs         # agent daemon (connects to core via TCP)
├── core/                # shared runtime: server, db, tcp, config_storage
├── agent/               # agent: tcp client, runner, store
├── crc64.rs             # Java-compatible CRC64 (used for all ID computation)
├── core_agent_api.rs    # serde structs shared between core and agent
└── core/db.rs           # SQLite (sqlx) — all static SQL uses trace_sql! macro
crates/remote-file-source/  # SFTP/FTP input resolution
pm-admin/                # React + Ant Design UI
```

## ID Computation (Important)

All IDs (`agent_id`, `group_id`, `unit_id`, `strategy_id`) use `crc64_ecma(input_string)`.

- **agent_id** = `crc64_ecma("{agent_ip}_{deploy_dir}")`
- **group_id** = `crc64_ecma(group_name)`
- **unit_id** = `crc64_ecma(unit_name)`

CRC64 values exceed `Number.MAX_SAFE_INTEGER`. The backend serialises all `i64` ID fields as JSON **strings** (`serde_i64` module in `core_agent_api.rs`). Frontend types use `string` for all IDs — do not convert to `number`.

## Commands

```bash
cargo test                         # full suite (~62 unit tests)
cargo test <name>                  # focused test
cargo build --release              # builds core + agent + parser
cargo build --release --locked     # before push
cargo build --release --locked --target x86_64-unknown-linux-musl  # CI artifact
```

No lint, formatter, Clippy, or pre-commit wired into CI.

## After Rust Changes

```bash
cargo build --release && \
cp target/release/core test/core && \
cp target/release/agent test/agent && \
cp server.toml agent.toml test/
```

## Frontend (pm-admin/)

```bash
npm run build    # tsc -b && vite build
npm run lint     # oxlint
```

Pre-existing lint warning: `react-hooks/exhaustive-deps` in `FormPage.tsx` about `useMemo` deps on `configNames`. Ignore unless modifying that file.

### Table layout conventions
- List pages: `<div className="table-scroll-wrap">` → `<Table className="data-table">`
- With card head: `<div className="table-scroll-wrap with-card-head">`
- Scroll: `scroll={{ x: 'max-content', y: 'var(--table-scroll-y)' }}`
- Do NOT apply to `Results/GridTable.tsx` (has its own layout).

### Agent selectors
All agent dropdowns show `agent_alias` (with `agent_name` fallback). Group selectors show `{group_name} [机组]`.

## DB Logging

All DB methods in `src/core/db.rs` must log SQL + parameters. Use the `trace_sql!` macro for static-SQL calls (auto-generates `[db] ==> SQL` + `[db] ==> Parameters:` lines). For dynamic-SQL methods (e.g. `update_strategy`, `list_strategies`, `update_agent_info`), use `tracing::info!` directly.

## Architecture Notes

- **Core** (`src/bin/core.rs`): Axum HTTP (`/api/...`) + TCP agent registry. Routes defined in `server.rs::router()`.
- **Agent** (`src/bin/agent.rs`): connects to core via TCP, reports heartbeat, receives tasks.
- `compute_agent_id` in `agent_id.rs` — used by `listener.rs` (registry key) and `server.rs` dispatch loop.
- Agent auto-sends `deploy_dir` (from `std::env::current_exe()`) on registration.
- `agent_alias` auto-filled on first register only: `{ip_第三段}.{ip_第四段}.{counter:02d}`. Checked by `get_agent_alias` before recomputing.
- `data_collector_unit.agent_ids` can contain agent IDs **or** group IDs (no prefix). Backend validates against both `agent_info` and `agent_group` tables.

## Git / Generated Files

Gitignored: `/target/`, `/output/`, `/valid/`, `/downloads/`, `/rules/`, `fixtures/*.gz`, `fixtures/*.zip`.

`valid1/` is tracked (checked baseline). Do not overwrite. Keep secret-bearing config out of commits.
