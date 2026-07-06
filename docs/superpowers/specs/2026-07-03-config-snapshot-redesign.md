# Config Snapshot Redesign

## Goal

Replace the current stub-based config snapshot system with a production-ready design:

1. Admin uploads a config zip via HTTP API → Core validates file completeness → stores as versioned snapshot.
2. Core runs an SFTP server (or uses the host OS SFTP) to expose config files; Agents download via SFTP.
3. Full version management: activate any version, rollback on anomaly.

## Motivation

The current `GET /api/config-snapshots/{id}` is a stub. The Agent `--config-dir` workaround does not scale beyond one machine. A dedicated config management pipeline is needed for multi-Agent deployments.

## Config Storage Layout

```
config_storage/                         # root, configurable path
  versions/
    v1_20260703_120000_abc123/          # each version is a directory
      source.toml
      mapping_dx.ini
      load.toml
      colNameCutConfig.ini              # optional
      rules/
        rule_a.json
    v2_20260703_140000_def456/
      ...
  active -> v1_20260703_120000_abc123   # symlink, always points to active version
```

A version directory is the single source of truth. Core never serves stale files because the symlink is atomically swapped.

## Zip Upload & Validation

### Endpoint

```
POST /api/config-snapshots/upload
Content-Type: multipart/form-data
Body: file=<zip>
```

### Validation Rules

The zip **must** contain these files at the zip root:

| File | Required | Notes |
|------|----------|-------|
| `source.toml` | Yes | FTP/SFTP source config |
| `mapping_dx.ini` | Yes | Table/column mapping |
| `load.toml` | Yes | DB load config |
| `rules/` directory | Yes | May be empty (no TPD rules) |
| `colNameCutConfig.ini` | No | Column name normalization |

Validation returns a structured response:

```json
{
  "valid": false,
  "errors": [
    "missing required file: source.toml",
    "missing required file: mapping_dx.ini"
  ]
}
```

On success:

```json
{
  "valid": true,
  "config_snapshot_id": "v1_20260703_120000_abc123",
  "content_hash": "sha256:abc123...",
  "file_count": 5
}
```

### Upload Flow

1. Receive zip in memory.
2. List zip entries (no disk write yet).
3. Check required files. If any missing → return 400 with error list.
4. Unpack zip to `config_storage/versions/{snapshot_id}/`.
5. Compute `content_hash`: sha256 of the sorted list of `(relative_path, file_content)` pairs, each pair serialized as `{path}\0{content}\0`. This ensures deterministic hashing regardless of filesystem iteration order or zip entry order.
6. Insert metadata row into `config_snapshots` table.
7. Return success.

## Core API

All endpoints under `/api/config-snapshots`:

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/api/config-snapshots/upload` | Upload zip, validate, create version. Does NOT auto-activate. |
| GET | `/api/config-snapshots` | List all versions (id, created_at, content_hash, is_active). |
| GET | `/api/config-snapshots/{id}` | Return metadata about a specific version. |
| GET | `/api/config-snapshots/{id}/download` | Download the version as a zip archive. |
| POST | `/api/config-snapshots/{id}/activate` | Set this version as active. Atomically swap symlink + notify Agents. |
| DELETE | `/api/config-snapshots/{id}` | Delete a version (must not be active). |

### Activate & Notification

When `/activate` is called:

1. Validate that `{id}` exists.
2. Atomically swap the `active` symlink to point to `config_storage/versions/{id}/`.
3. Update `config_snapshots` DB row: set `is_active = 1` for this id, `is_active = 0` for all others.
4. Fetch all ONLINE agents from `agents` table. Each row contains `agent_id`, `host`, `port` (populated during agent registration).
5. For each agent, POST to `http://{agent_host}:{agent_port}/api/config/notify` with:
   ```json
   {
     "config_snapshot_id": "{id}",
     "content_hash": "sha256:...",
     "sftp_path": "/path/on/sftp/server/{id}"
   }
   ```
6. Collect per-agent notification results and return in response:
   ```json
   {
     "config_snapshot_id": "{id}",
     "active": true,
     "agents_notified": 3,
     "agents_succeeded": 2,
     "agent_results": [
       {"agent_id": "agent_1", "status": "succeeded"},
       {"agent_id": "agent_2", "status": "failed", "error": "connection timeout"},
       {"agent_id": "agent_3", "status": "succeeded"}
     ]
   }
   ```

### Rollback

Rollback is simply activating an older version:

```
POST /api/config-snapshots/v1_20260703_120000_abc123/activate
```

No separate rollback endpoint is needed.

## Core SFTP Access

Config files live on the Core machine's filesystem under `config_storage/versions/{id}/`.

Agents connect via SFTP to the Core machine to download config files. The Core machine must have an SFTP server (e.g., openssh-server) running that has read access to the config storage directory.

Configuration required on the Core machine:
- SFTP user with read access to `config_storage/`
- A dedicated chroot directory or a symlink from an SFTP-accessible path to `config_storage/`

For first-version simplicity, the Admin configures the SFTP paths manually. The Agent does not discover the SFTP path from Core — it is part of the Agent's static `config_sftp.toml`.

The Core machine must configure its SFTP server to expose the `config_storage/` directory (or a symlink to it) at a known path such as `/var/sftp/config_storage/`. This path becomes the `remote_root` in the Agent's `config_sftp.toml`.

## Agent Config Distribution

### config_sftp.toml

A new file co-located with the Agent binary or specified via `--config-sftp` flag:

```toml
[config_sftp]
type = "sftp"
enabled = true
remote_root = "/var/sftp/config_storage"
check_interval_seconds = 300

[config_sftp.connection]
host = "10.0.0.10"
port = 22
username = "config-sync"
password = "..."
```

### Agent Endpoint

```
POST /api/config/notify
```

Accepts:

```json
{
  "config_snapshot_id": "v1_20260703_120000_abc123",
  "content_hash": "sha256:abc123...",
  "sftp_path": "/versions/v1_20260703_120000_abc123"
}
```

Agent flow:

1. Receive notification.
2. Create `agent_data/config_snapshots/{id}/.downloading` marker.
3. Connect via SFTP using `config_sftp.toml` credentials.
4. Recursively download all files from `{remote_root}/{sftp_path}` to `agent_data/config_snapshots/{id}/`.
5. Verify content hash against downloaded files.
6. Remove `.downloading` marker, atomically rename if needed.
7. Return 200 to Core.

If verification fails, Agent deletes the partial download and returns 400 with error details.

### Agent Config Caching Layout

```
agent_data/
  config_snapshots/
    v1_20260703_120000_abc123/
      source.toml
      mapping_dx.ini
      load.toml
      rules/
    v2_20260703_140000_def456/
      ...
```

When a task is dispatched, it references `config_snapshot_id`. The Agent checks `agent_data/config_snapshots/{id}/`. If found, use it. If not found, raise an error (the config should have been pushed before task dispatch).

### Periodic Check

In addition to push notifications, the Agent periodically checks (per `check_interval_seconds`) whether the active config changed. It does this by reading the target of the `active` symlink on the SFTP server via `readlink`. If the target differs from the currently cached snapshot ID, the Agent fetches the new active version.

## Version Management

### DB Schema (existing `config_snapshots` table, updated)

```sql
CREATE TABLE IF NOT EXISTS config_snapshots (
    config_snapshot_id TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    version_label TEXT,           -- new: human-readable label, e.g., "v1"
    is_active INTEGER NOT NULL DEFAULT 0,  -- new: 1 if currently active
    file_count INTEGER NOT NULL DEFAULT 0, -- new: number of files in zip
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    activated_at TEXT             -- new: when this version was activated
);
```

### Retention

Keep all versions. Admin can delete non-active versions via `DELETE /api/config-snapshots/{id}` which removes both the DB row and the filesystem directory.

## Error Handling

### Upload Errors

| Condition | HTTP | Response |
|-----------|------|----------|
| Missing required file | 400 | `{"valid":false,"errors":["missing source.toml",...]}` |
| Invalid zip format | 400 | `{"valid":false,"errors":["invalid zip: ..."]}` |
| Disk write failure | 500 | `{"valid":false,"errors":["write error: ..."]}` |

### Activate Errors

| Condition | HTTP | Response |
|-----------|------|----------|
| Snapshot not found | 404 | error detail |
| Already active | 200 | no-op response |
| Some agents failed | 200 | partial success with agent results |

## Implementation Plan

### Phase 1: Core API + Validation (no SFTP distribution yet)

1. Update `config_snapshots` DB schema (add `is_active`, `version_label`, `file_count`, `activated_at`).
2. Implement `POST /api/config-snapshots/upload` with zip validation and disk unpack.
3. Implement `GET /api/config-snapshots` listing endpoint.
4. Implement `GET /api/config-snapshots/{id}` metadata endpoint.
5. Implement `POST /api/config-snapshots/{id}/activate` with symlink swap.

### Phase 2: Agent Config Fetch via SFTP

1. Implement Agent `config_sftp.toml` parsing.
2. Implement SFTP recursive download for config snapshots.
3. Implement Agent `POST /api/config/notify` handler.
4. Wire notification from Core's activate handler to Agents.
5. Add periodic sync as fallback.

### Phase 3: Rollback & Admin UI

1. Test rollback via activate of old version.
2. Web upload page (future, not part of this implementation plan).
