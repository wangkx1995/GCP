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

## Create Config Snapshot

```bash
curl -sS -X POST http://127.0.0.1:18080/api/config/snapshots \
  -H 'content-type: application/json' \
  -d '{"config_snapshot_id":"cfg_001","content_hash":"sha256:test","source_toml":"[source]\nhost = \"127.0.0.1\"","mapping_dx_ini":"[mapping]","load_toml":"[load]","col_name_cut_config_ini":null,"rules":[{"relative_path":"rules/a.json","content":"{\"table_name\":\"TPD_A\"}"}]}'
```

## Create Task

```bash
curl -sS -X POST http://127.0.0.1:18080/api/tasks \
  -H 'content-type: application/json' \
  -d '{"task_id":"task_001","logical_task_key":"strategy:2026-06-17 15:15:00:cfg","strategy_id":"strategy_1","config_snapshot_id":"cfg_001","scan_start_time":"2026-06-17 15:15:00","collect_id":"collect_001","load_type":"clickhouse","encoding":"UTF-8","output_delimiter":"|","timeout_seconds":1800,"callback_base_url":"http://127.0.0.1:18080/api"}'
```

## Dispatch Task

```bash
curl -sS -X POST http://127.0.0.1:18081/api/tasks \
  -H 'content-type: application/json' \
  -d '{"task_id":"task_001","logical_task_key":"strategy:2026-06-17 15:15:00:cfg","strategy_id":"strategy_1","config_snapshot_id":"cfg_001","scan_start_time":"2026-06-17 15:15:00","collect_id":"collect_001","load_type":"clickhouse","encoding":"UTF-8","output_delimiter":"|","timeout_seconds":1800,"callback_base_url":"http://127.0.0.1:18080/api"}'
```

## Query Result Grid

```bash
curl -sS 'http://127.0.0.1:18080/api/results/grid?strategy_id=strategy_1&day=2026-06-17&interval_minutes=15'
```

The grid response contains `time_slots` and one row per table. Cells are colored `green`, `yellow`, `red`, or `gray`.
