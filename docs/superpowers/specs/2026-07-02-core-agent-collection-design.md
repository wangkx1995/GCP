# Core/Agent Collection Design

## Goal

Evolve the current single-node PM parser into a core-machine plus multi-Agent collection system.

The first version uses Core-driven HTTP task dispatch because the expected Agent count is under 10 and all Agents are in the same network segment. The design keeps the task model asynchronous so the dispatch layer can later move to MQ with limited impact.

## Scope

In scope for the first version:

- Core manages Agent registration, heartbeat, collection configuration, collection strategies, task dispatch, task state, and result display.
- Agent receives tasks from Core, runs collection and parsing, generates existing output packages, reads `result.csv`, and reports results to Core.
- Core stores `result.csv` rows and provides a daily table/time-grid query page with colored status cells.
- Configuration files such as `source.toml`, `mapping_dx.ini`, `load.toml`, `rules/*.json`, and `colNameCutConfig.ini` are managed by Core as versioned config snapshots.

Out of scope for the first version:

- Agent-side database loading.
- MQ dispatch.
- File-level hash verification of output packages.
- Remote directory re-scan by Core for independent verification.
- Complex auto-balancing, Agent upgrade management, or alerting.

## High-Level Architecture

Components:

- `Core Server`: owns configuration, strategies, task state, Agent state, result storage, and query UI.
- `Agent`: exposes HTTP endpoints for task dispatch and status queries, executes tasks asynchronously, and reports status/results to Core.
- `Parser Executor`: reuses the existing Rust parser flow for remote/local PM files, TPD aggregation, and output package generation.

Boundary:

- Core is responsible for control and visibility.
- Agent is responsible for execution.
- Parser Executor is responsible for collection/parsing/output generation.

First version dispatch flow:

```text
Core creates task
Core chooses Agent
Core POSTs task to Agent
Agent persists task locally
Agent returns accepted immediately
Agent runs task in background
Agent reports progress/status to Core
Agent reports parsed result.csv rows to Core
Core stores results and displays collection grid
```

The HTTP task dispatch must not block until parsing completes. It only means "task accepted".

## Future MQ Compatibility

The first version should isolate task dispatch behind a small boundary:

```text
TaskDispatcher
  HttpTaskDispatcher now
  MqTaskDispatcher later
```

Agent-side receiving should have the same shape:

```text
TaskReceiver
  HttpTaskReceiver now
  MqTaskConsumer later
```

These should remain stable when moving from HTTP to MQ:

- Task schema.
- Task state machine.
- Agent execution flow.
- Core result storage.
- Core status/reporting endpoints.
- The `result.csv` based collection grid.

## Config Snapshots

Core is the source of truth for runtime configs:

- `source.toml`
- `mapping_dx.ini`
- `load.toml`
- `rules/*.json`
- `colNameCutConfig.ini`

Core stores these as `config_snapshots`. Each task references one `config_snapshot_id`.

Agent does not maintain long-lived independent parser configuration. On task execution it downloads or reuses the referenced config snapshot, writes it into the task work area, and runs the parser with those files.

Recommended Agent layout:

```text
agent_data/
  agent.json
  config_snapshots/
    cfg_001/
      snapshot.json
      source.toml
      mapping_dx.ini
      load.toml
      colNameCutConfig.ini
      rules/
        rule_a.json
  tasks/
    task_001/
      task.json
      state.json
      events.log
      config -> ../../config_snapshots/cfg_001
      downloads/
      output/
      logs/
      error.json
```

Config snapshot fetch:

```http
GET /api/config-snapshots/{config_snapshot_id}
```

The response includes the config file contents and a `content_hash`. Agent writes snapshots through a temporary directory and atomically renames after hash verification.

## Task Model

Core task fields:

- `task_id`
- `logical_task_key`
- `strategy_id`
- `config_snapshot_id`
- `scan_start_time`
- `collect_id`
- `assigned_agent_id`
- `attempt_no`
- `status`
- `created_at`
- `accepted_at`
- `started_at`
- `last_progress_at`
- `finished_at`
- `timeout_at`
- `error_code`
- `error_message`

Use `logical_task_key` to prevent duplicate active tasks:

```text
strategy_id + scan_start_time + config_snapshot_id
```

Core should allow retry history while ensuring only one non-terminal task exists for a logical key.

## Core Task States

Recommended first-version states:

- `CREATED`
- `DISPATCHING`
- `ACCEPTED`
- `RUNNING`
- `SUCCEEDED`
- `FAILED`
- `TIMEOUT`
- `CANCEL_REQUESTED`
- `CANCELLED`

`SUCCEEDED` means the Agent completed parsing and Core accepted the reported `result.csv` rows. There is no separate heavy verification phase in version one.

Terminal states:

- `SUCCEEDED`
- `FAILED`
- `TIMEOUT`
- `CANCELLED`

Terminal tasks are not overwritten by late normal status reports. Late events may be recorded for audit/debugging.

## Agent Execution States

Agent-local states can be more detailed:

- `ACCEPTED`
- `PREPARING_CONFIG`
- `DOWNLOADING`
- `PARSING`
- `WRITING_OUTPUT`
- `REPORTING_RESULT`
- `SUCCEEDED`
- `FAILED`
- `CANCELLED`

Agent reports these to Core as `RUNNING` with a `phase`, except terminal states.

Agent should persist a task before returning `accepted` to Core. This prevents task loss if Agent crashes immediately after accepting.

## HTTP Interfaces

Agent to Core:

- `POST /api/agents/register`: register or reconnect Agent.
- `POST /api/agents/{agent_id}/heartbeat`: report liveness and current running tasks.
- `GET /api/config-snapshots/{config_snapshot_id}`: fetch config snapshot.
- `POST /api/tasks/{task_id}/events`: report accepted/running/progress/failed/cancelled events.
- `POST /api/tasks/{task_id}/result`: report successful `result.csv` rows.

Core to Agent:

- `POST http://{agent_host}:{agent_port}/api/tasks`: dispatch task, returns accepted quickly.
- `GET http://{agent_host}:{agent_port}/api/tasks/{task_id}`: query Agent-local task state.
- `POST http://{agent_host}:{agent_port}/api/tasks/{task_id}/cancel`: request soft cancellation.

The dispatch endpoint is idempotent by `task_id`.

## Parser Reuse

The current parser should be reusable by Agent. Preferred implementation path:

1. Extract the current `main.rs` parse workflow into a library function such as `run_parse_job(options)`.
2. Keep the existing CLI by making it call `run_parse_job`.
3. Make Agent call `run_parse_job` directly.

The Agent maps task/config data to the same parser inputs used today:

```text
--source-config task/config/source.toml
--scan-start-time task.scan_start_time
--config-dir task/config
--output-dir task/output
--collect-id task.collect_id
--load-type task.load_type
--load-config task/config/load.toml
--encoding task.encoding
--rules-dir task/config/rules
```

For a faster MVP, Agent could initially spawn the existing parser binary as a subprocess, but the preferred stable design is a shared library function.

## Cancellation

Cancellation is intentionally simple in the first version.

Core sends a cancel request and marks the task `CANCEL_REQUESTED`. Agent sets a local cancellation flag.

Rules:

- If the task has not started, Agent marks it `CANCELLED` and reports that to Core.
- If the task is running, Agent stops at a safe phase boundary when possible.
- The first version does not require hard-killing parser work.
- If cancellation cannot stop the task promptly, the task may still finish as `SUCCEEDED` or `FAILED`.
- Cancelled tasks do not report `result.csv` rows.

Agent must still report a terminal status. It should not silently stop without reporting, because Core would be unable to distinguish cancellation from crash or network loss.

## Result Collection

The current parser writes `result.csv` with these columns:

```csv
table_name,data_time,row_count,success,collect_time
```

Agent success flow:

1. Parser finishes successfully.
2. Agent scans the output directory for all `result.csv` files.
3. Agent reads all rows.
4. Agent reports rows to Core.
5. Core inserts rows into its database.
6. Core marks the task `SUCCEEDED`.

Example result report:

```json
{
  "task_id": "task_001",
  "agent_id": "agent_001",
  "status": "SUCCEEDED",
  "result_rows": [
    {
      "table_name": "TPD_A",
      "data_time": "2026-06-17 15:15:00",
      "row_count": 100,
      "success": 1,
      "collect_time": "2026-07-02 15:35:00"
    }
  ]
}
```

No complex manifest is required in the first version.

## Core Result Storage

Recommended table: `collect_result_cells`.

Fields:

- `id`
- `task_id`
- `strategy_id`
- `agent_id`
- `config_snapshot_id`
- `table_name`
- `data_time`
- `row_count`
- `success`
- `collect_time`
- `status`
- `error_message`
- `created_at`
- `updated_at`

Keep task history. Do not overwrite old retry attempts blindly. The query page can choose latest attempt or latest successful attempt per cell.

## Collection Grid Page

The query page displays a daily matrix:

- Rows: destination table names.
- Columns: collection time slots for the selected day.
- Cell value: collected row count or status marker.
- Cell color: collection status.

For 15-minute granularity, the page generates 96 columns from `00:00` to `23:45`.

Table names should come from the selected config snapshot's `rules/*.json` `table_name` values where practical. This allows the page to show missing tables/time slots, not only existing result rows.

Suggested colors:

- Green: `success=1` and `row_count > 0`.
- Yellow: `success=1` and `row_count = 0`.
- Red: task failed or `success=0`.
- Gray: no result for the expected table/time slot.
- Blue: task is running or accepted.
- Orange: task timed out or needs retry.

This page is the first-version completeness check. It answers which tables and time slots collected data and which appear abnormal.

## Scheduling

First-version Agent selection rules:

1. Agent must be `ONLINE`.
2. Agent capabilities must match task needs.
3. Prefer the Agent with the fewest running tasks.
4. If the user selected an Agent, use that Agent first.

Heartbeat status:

- `ONLINE`: heartbeat within 3 heartbeat intervals.
- `UNKNOWN`: heartbeat missing for more than 3 intervals.
- `OFFLINE`: heartbeat missing for more than 6 intervals.

Default Agent concurrency should be conservative:

```text
max_concurrent_tasks = 1
```

The value can be made configurable later.

## Retry and Timeout

First version retries whole tasks, not partial parser state.

On failure or timeout, Core may create a retry task with:

- same `logical_task_key`
- incremented `attempt_no`
- new `task_id`
- `original_task_id` pointing to the failed attempt

Agent restart recovery:

- In-progress local tasks become failed with `error_code=AGENT_RESTARTED`.
- Completed tasks with unreported results may retry result reporting.
- Half-finished parser work is not resumed.

## Implementation Notes

- Keep the existing single-node CLI working.
- Start by extracting parser execution from `main.rs` into a reusable function.
- Add Core and Agent as additional binaries in the same Rust workspace unless there is a strong reason to split repositories.
- Store config snapshots in Core and materialize them as files on Agent to match the existing parser expectations.
- Keep result ingestion based on `result.csv` rows, not full output package inspection.
