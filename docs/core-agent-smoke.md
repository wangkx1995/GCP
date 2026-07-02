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
