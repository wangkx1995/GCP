# Config Agent Distribution — Phase 2

## Goal

Let Agents fetch Core-managed config snapshots (uploaded via Phase 1) without relying on shared filesystem or SFTP. Two sub-features:

- **B (HTTP 直连):** Agent downloads config from Core via HTTP on demand
- **C (热更新通知):** Core notifies Agents when a config is activated, Agents pull asynchronously

## Architecture

```
                    activate v2
  Core ──────────────────────────────────┐
  (POST /api/config-snapshots/v2/         │
        activate)                         │
         │                                ▼
         │  GET /api/config-snapshots  ┌──────────┐
         │    /{id}/download           │ Agent A  │
         │  ◄───────────────────────── │          │
         │      (B, on task receipt)   │ data_dir/│
         │                             │  configs/│
         │  POST /api/configs/update   │  v1/     │
         │  ──────────────────────────►│  v2/     │
         │      (C, on activate)       └──────────┘
         ▼
  DB: config_snapshots
  FS: config_storage/versions/
```

## B: HTTP 直连分发

### Trigger

Agent receives a task via `POST /api/tasks`. During `persist_task` processing:

1. Check `data_dir/configs/{snapshot_id}/` for required files:
   - `source.toml`
   - `mapping_dx.ini`
   - `load.toml`
   - `rules/` directory with at least one file
2. If any file missing → download from Core:
   - `GET {core_api_base}/api/config-snapshots/{id}/download`
   - Response is raw zip bytes (`application/zip`)
3. Unpack zip to `data_dir/configs/{snapshot_id}/`
4. Proceed with normal task execution

### Caching

- Configs are cached at `data_dir/configs/{snapshot_id}/` indefinitely
- No re-download if all required files exist
- Agent restart preserves cache (persistent on disk)

### Error Handling

- Download failure → task marked FAILED with descriptive error
- Invalid zip (extraction fails) → task marked FAILED
- Network timeout → 30s default, retry 1x

### --config-dir Override

- If `--config-dir` is provided at Agent startup, that directory is used directly
- HTTP download is skipped entirely for debugging/local development
- Determined at `persist_task` time: if `config_dir` is set, use it; otherwise use HTTP

## C: 热更新通知

### Core Side

Added at the end of `activate_config_snapshot` handler, after symlink swap succeeds:

```
let online_agents = db.list_online_agents()?;
for agent in online_agents {
    tokio::spawn(async move {
        let url = format!("http://{}:{}/api/configs/update", agent.host, agent.port);
        let body = json!({"snapshot_id": id, "content_hash": hash});
        match http.post(url).json(&body).timeout(Duration::from_secs(5)).send().await {
            Ok(_) => tracing::info!("notified agent {} of config {}", agent.agent_id, id),
            Err(e) => tracing::warn!("failed to notify agent {}: {}", agent.agent_id, e),
        }
    });
}
```

- Non-blocking: Core returns 200 to the admin caller immediately
- Failures logged as warnings only
- No retry for offline agents (they get config via B when they come online)

### Agent Side

New endpoint `POST /api/configs/update`:

```
Request: { "snapshot_id": "...", "content_hash": "sha256:..." }
Response: 200 { "accepted": true }
```

Handler:
1. Return 200 immediately
2. Spawn async task:
   a. Skip if `data_dir/configs/{snapshot_id}/` already exists with matching hash
   b. Download zip: `GET {core_api_base}/api/config-snapshots/{id}/download`
   c. Unpack to `data_dir/configs/{snapshot_id}/`
   d. Compute content hash and verify against received hash
   e. Log result

### Impact on Running Tasks

- None. Running tasks continue with the config they started with
- The new config only applies to tasks dispatched after activation
- The field `current_active_config` is informational only (for logging/debug)

### Concurrency: B and C Race

If an Agent receives both a task dispatch (triggering B) and a config update notification (triggering C) for the same `snapshot_id` simultaneously:

- The download is idempotent: if the directory already exists with matching hash, the second caller skips
- If both reach the download step concurrently, both download the same zip and write to the same directory — the second write simply overwrites the first with identical content
- No locking needed; the "check existence then download" sequence is best-effort, not transactional
- A lock per `snapshot_id` can be added later if concurrent downloads become a real issue

## Data Flow Summary

```
Admin uploads zip ──► POST /api/config-snapshots/upload
                           │
                           ▼
                    Core validates + unpacks + DB insert
                           │
Admin activates ────► POST /api/config-snapshots/v2/activate
                           │
                    ┌──────┴──────┐
                    ▼              ▼
              Core symlink    Core notifies Agents (C)
              swap + DB           │
              update              ▼
                           Agent pulls via HTTP (B)
                           + verifies hash
                                   
Agent gets task ───► POST /api/tasks
                           │
                    Check local config cache (B)
                           │
                    If missing: HTTP download from Core
                           │
                    Run parse job with local config
```

## Configs Required

| Config | Phase | Description |
|--------|-------|-------------|
| Core API base URL | B | Already exists as `--core-api-base` on Agent |
| Core download endpoint | B | Phase 1 already implements `GET /api/config-snapshots/{id}/download` |
| None new | B+C | Zero new config files |

## Testing

- **Unit:** Agent store test: `ensure_config` with mock HTTP server
- **Unit:** Agent configs update handler: receive notification, verify async download triggered
- **Unit:** Core activate handler: verify POST sent to mock agent
- **Integration:** Start real Core + Agent, upload config, activate, verify Agent downloads it

## Out of Scope (Phase 2)

- SFTP distribution (deferred to Phase 3 if needed)
- Hash mismatch recovery (re-download once on failure, hard fail on second)
- Web UI for config management
- Agent health check / config version reporting back to Core
