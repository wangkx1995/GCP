# Config Agent Distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let Agents fetch Core-managed config snapshots via HTTP (B) and receive hot-update notifications on activation (C).

**Architecture:** Two additions to the existing Core/Agent architecture. (B) Agent checks local config cache on task receipt, downloads from Core's existing download endpoint if missing. (C) Core's activate handler POSTs to all online Agents, Agent asynchronously pulls the config.

**Tech Stack:** Rust, reqwest, zip crate (already in deps)

## Global Constraints

- `--config-dir` on Agent takes priority over HTTP download (local dev override)
- Config cache at `data_dir/config_snapshots/{snapshot_id}/`, same layout as Core's version dirs
- Core notification to Agent is fire-and-forget (warn on failure, no retry)
- Running tasks are never interrupted by config updates
- Existing `--config-dir` behavior unchanged
- All existing tests must continue to pass

---

### Task 1: Core — add list_online_agents DB method + activate notification to Agents

**Files:**
- Modify: `src/core/db.rs` (add `list_online_agents` method)
- Modify: `src/core/server.rs` (modify `activate_config_snapshot` handler to notify agents)

**Interfaces:**
- Consumes: existing `CoreDb`, existing `activate_config_snapshot` handler structure
- Produces: `CoreDb::list_online_agents() -> Result<Vec<OnlineAgent>>` where `OnlineAgent { agent_id, host, port }`

- [ ] **Step 1: Write tests for `list_online_agents`**

Add to test module in `src/core/db.rs`:

```rust
#[test]
fn lists_online_agents() {
    let db = db();
    let mut req = agent_request();
    req.agent_id = Some("agent_a".into());
    db.register_agent(&req).unwrap();
    // agent_a is ONLINE after register

    let agents = db.list_online_agents().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].agent_id, "agent_a");
    assert_eq!(agents[0].host, "127.0.0.1");
    assert_eq!(agents[0].port, 18081);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib db::tests::lists_online_agents -v`
Expected: compilation error (struct `OnlineAgent` and method don't exist)

- [ ] **Step 3: Add `OnlineAgent` struct and `list_online_agents` method**

Add to `src/core_agent_api.rs`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnlineAgent {
    pub agent_id: String,
    pub host: String,
    pub port: u16,
}
```

Add to `src/core/db.rs`:

```rust
use crate::core_agent_api::OnlineAgent;

pub fn list_online_agents(&self) -> Result<Vec<OnlineAgent>> {
    let mut stmt = self.conn.prepare(
        "SELECT agent_id, host, port FROM agents WHERE status = 'ONLINE'"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(OnlineAgent {
            agent_id: row.get(0)?,
            host: row.get(1)?,
            port: row.get(2)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib db::tests::lists_online_agents -v`
Expected: PASS

- [ ] **Step 5: Modify activate handler to notify Agents**

In `src/core/server.rs`, modify `activate_config_snapshot` handler. After the symlink swap succeeds and before returning OK, add notification logic. Add import:

```rust
use std::time::Duration;
use crate::core_agent_api::OnlineAgent;
```

Replace the existing `activate_config_snapshot` handler with:

```rust
async fn activate_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let meta = state.db.lock().await.get_config_snapshot(&id)
        .map_err(|_| (StatusCode::NOT_FOUND, format!("snapshot {id} not found")))?;

    let target = state.storage.version_dir(&id);
    if !target.exists() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("version dir {} missing", target.display())));
    }

    let active = state.storage.active_link();
    let temp = active.with_extension("tmp");
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&temp);
        std::os::unix::fs::symlink(&target, &temp).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("symlink error: {e}")))?;
        std::fs::rename(&temp, &active).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("rename error: {e}")))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&active, target.to_string_lossy().as_bytes())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write error: {e}")))?;
    }

    let meta = state.db.lock().await.activate_config_snapshot(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    tracing::info!("[core] activated config snapshot {id}");

    // Notify online agents (C)
    let agents = match state.db.lock().await.list_online_agents() {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("[core] failed to list online agents: {e}");
            Vec::new()
        }
    };
    let content_hash = meta.content_hash.clone();
    for agent in &agents {
        let http = state.http.clone();
        let agent_id = agent.agent_id.clone();
        let url = format!("http://{}:{}/api/configs/update", agent.host, agent.port);
        let body = serde_json::json!({
            "snapshot_id": id,
            "content_hash": content_hash,
        });
        tokio::spawn(async move {
            match http.post(&url).json(&body).timeout(Duration::from_secs(5)).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        tracing::info!("[core] notified agent {agent_id} of config {id}");
                    } else {
                        tracing::warn!("[core] agent {agent_id} rejected config update: {}", resp.status());
                    }
                }
                Err(e) => tracing::warn!("[core] failed to notify agent {agent_id}: {e}"),
            }
        });
    }

    Ok(Json(serde_json::json!({
        "config_snapshot_id": id,
        "active": true,
        "content_hash": meta.content_hash,
        "activated_at": meta.activated_at,
    })))
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test --lib`
Expected: all tests pass

- [ ] **Step 7: Commit**

```bash
git add src/core/db.rs src/core/server.rs src/core_agent_api.rs
git commit -m "feat(config-dist): add list_online_agents + activate notification to agents"
```

---

### Task 2: Agent — add core_api_base to Store, ensure_config, modify persist flow

**Files:**
- Modify: `src/agent/store.rs` (add `core_api_base` field, `ensure_config`, modify `populate_config`)
- Modify: `src/agent/server.rs` (pass `core_api_base` to `AgentStore::new`, call `ensure_config` in handler)
- Modify: `src/bin/agent.rs` (no change — already passes `core_api_base`)
- Modify: `src/agent/runner.rs` (no change needed for this task)

**Interfaces:**
- Consumes: `AgentStore::new(root, config_dir)` → `AgentStore::new(root, config_dir, core_api_base)`
- Produces: `AgentStore::ensure_config(snapshot_id, http) -> Result<PathBuf>` (downloads if not cached)

- [ ] **Step 1: Write tests for ensure_config**

In `src/agent/store.rs` test module:

```rust
#[test]
fn uses_local_cache_if_present() {
    use std::io::Write;
    let dir = tempdir().unwrap();
    let store = AgentStore::new(dir.path().join("agent_data"), None, "http://core/api".to_string()).unwrap();

    // Pre-populate cache
    let cache_dir = dir.path().join("agent_data/config_snapshots/cfg_v1");
    std::fs::create_dir_all(cache_dir.join("rules")).unwrap();
    std::fs::write(cache_dir.join("source.toml"), b"[source]").unwrap();
    std::fs::write(cache_dir.join("mapping_dx.ini"), b"[m]").unwrap();
    std::fs::write(cache_dir.join("load.toml"), b"[l]").unwrap();
    std::fs::write(cache_dir.join("rules/a.json"), b"{}").unwrap();

    // Run synchronously — it should find the cache and not attempt HTTP
    // We test the cache-check path; HTTP path is tested in server tests
    let result = store.ensure_config_sync("cfg_v1");
    assert!(result.is_ok());
    assert!(result.unwrap().join("source.toml").exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib agent::store::tests::uses_local_cache_if_present -v`
Expected: compilation error (`ensure_config_sync` doesn't exist)

- [ ] **Step 3: Implement AgentStore changes**

In `src/agent/store.rs`:

Add `core_api_base` field and update constructor:

```rust
#[derive(Clone, Debug)]
pub struct AgentStore {
    root: PathBuf,
    config_dir: Option<PathBuf>,
    core_api_base: String,
}

impl AgentStore {
    pub fn new(root: PathBuf, config_dir: Option<PathBuf>, core_api_base: String) -> Result<Self> {
        std::fs::create_dir_all(root.join("tasks"))?;
        std::fs::create_dir_all(root.join("config_snapshots"))?;
        if let Some(ref cfg) = config_dir {
            if !cfg.exists() {
                anyhow::bail!("config-dir {} does not exist", cfg.display());
            }
        }
        Ok(Self { root, config_dir, core_api_base })
    }
```

Add `ensure_config` method:

```rust
pub fn ensure_config_sync(&self, snapshot_id: &str) -> Result<PathBuf> {
    use std::io::Read;
    let config_root = self.root.join("config_snapshots").join(snapshot_id);
    let marker = config_root.join("source.toml");
    if marker.exists() {
        tracing::info!("[agent-store] config {} already cached at {}", snapshot_id, config_root.display());
        return Ok(config_root);
    }
    anyhow::bail!("config {} not cached and async download not available in sync path; call ensure_config_async from async context", snapshot_id)
}

pub async fn ensure_config_async(&self, snapshot_id: &str, http: &reqwest::Client) -> Result<PathBuf> {
    let config_root = self.root.join("config_snapshots").join(snapshot_id);
    let marker = config_root.join("source.toml");
    if marker.exists() {
        tracing::info!("[agent-store] config {} already cached", snapshot_id);
        return Ok(config_root);
    }

    // Download zip from Core
    let url = format!("{}/config-snapshots/{}/download", self.core_api_base, snapshot_id);
    tracing::info!("[agent-store] downloading config {} from {}", snapshot_id, url);
    let resp = http.get(&url).send().await
        .map_err(|e| anyhow::anyhow!("download config {snapshot_id}: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("download config {snapshot_id}: HTTP {}", resp.status());
    }
    let zip_data = resp.bytes().await?;

    // Unpack to config_root
    std::fs::create_dir_all(&config_root)?;
    let reader = std::io::Cursor::new(&zip_data);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| anyhow::anyhow!("invalid zip for {snapshot_id}: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().trim_end_matches('/').to_string();
        if file.is_dir() {
            std::fs::create_dir_all(config_root.join(&name))?;
        } else {
            let target = config_root.join(&name);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;
            std::fs::write(&target, &content)?;
        }
    }

    tracing::info!("[agent-store] unpacked config {} to {}", snapshot_id, config_root.display());
    Ok(config_root)
}
```

Modify `populate_config` to fall back to config cache:

```rust
fn populate_config(&self, task_dir: &Path, snapshot_id: &str) -> Result<()> {
    let dest = task_dir.join("config");
    let src = if let Some(ref cfg) = self.config_dir {
        cfg.clone()
    } else {
        self.root.join("config_snapshots").join(snapshot_id)
    };
    if !src.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&src)
        .with_context(|| format!("read config dir {}", src.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.file_name().map_or(true, |n| n == "rules") {
            if path.is_dir() {
                for rule_entry in std::fs::read_dir(&path)
                    .with_context(|| format!("read rules dir {}", path.display()))?
                {
                    let rule_entry = rule_entry?;
                    let rule_src = rule_entry.path();
                    if rule_src.is_file() {
                        let fname = rule_src.file_name().unwrap();
                        std::fs::copy(&rule_src, dest.join("rules").join(fname))
                            .with_context(|| format!("copy rule {}", rule_src.display()))?;
                    }
                }
            }
        } else if path.is_file() {
            std::fs::copy(&path, dest.join(path.file_name().unwrap()))
                .with_context(|| format!("copy config file {}", path.display()))?;
        }
    }
    tracing::info!("[agent-store] config files ready at {}", dest.display());
    Ok(())
}
```

Update `persist_task` to pass `snapshot_id`:

```rust
pub fn persist_task(&self, request: &TaskDispatchRequest) -> Result<PathBuf> {
    let task_dir = self.task_dir(&request.task_id);
    std::fs::create_dir_all(task_dir.join("downloads"))?;
    std::fs::create_dir_all(task_dir.join("output"))?;
    std::fs::create_dir_all(task_dir.join("logs"))?;
    std::fs::create_dir_all(task_dir.join("config"))?;
    std::fs::create_dir_all(task_dir.join("config").join("rules"))?;
    std::fs::write(task_dir.join("task.json"), serde_json::to_vec_pretty(request)?)?;
    self.write_state(&task_dir, TaskStatus::Accepted)?;
    self.populate_config(&task_dir, &request.config_snapshot_id)?;
    Ok(task_dir)
}
```

- [ ] **Step 4: Update tests in store.rs**

Fix existing test for new constructor:

```rust
#[test]
fn persists_task_before_execution() {
    let dir = tempdir().unwrap();
    let store = AgentStore::new(dir.path().join("agent_data"), None, "http://core/api".to_string()).unwrap();
    // ... rest unchanged
}
```

- [ ] **Step 5: Update server.rs — pass core_api_base + call ensure_config_async**

In `src/agent/server.rs`:

Update `run_agent_server`:

```rust
pub async fn run_agent_server(addr: SocketAddr, data_dir: PathBuf, core_api_base: String, agent_id: String, config_dir: Option<PathBuf>) -> Result<()> {
    let store = AgentStore::new(data_dir, config_dir, core_api_base.clone())?;
    let state = AgentState { store, runner: AgentRunner::new(agent_id, core_api_base) };
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
```

Update `dispatch_task` handler to call `ensure_config_async` before `persist_task`:

```rust
async fn dispatch_task(axum::extract::State(state): axum::extract::State<AgentState>, Json(request): Json<TaskDispatchRequest>) -> Json<TaskDispatchResponse> {
    let task_id = request.task_id.clone();
    tracing::info!("[agent-server] dispatch_task task_id={task_id} strategy_id={} scan_start_time={}", request.strategy_id, request.scan_start_time);

    if state.store.config_dir.is_none() {
        match state.store.ensure_config_async(&request.config_snapshot_id, &state.runner.http).await {
            Ok(path) => tracing::info!("[agent-server] config {} ready at {}", request.config_snapshot_id, path.display()),
            Err(e) => {
                tracing::error!("[agent-server] failed to ensure config: {e:#}");
                return Json(TaskDispatchResponse {
                    task_id, accepted: false,
                    agent_task_state: TaskStatus::Failed,
                    reason: Some(format!("config download failed: {e:#}")),
                });
            }
        }
    }

    match state.store.persist_task(&request) {
        // ... rest unchanged
    }
}
```

Note: `config_dir` is private. Make it accessible by adding a public method to AgentStore or check it differently. Simplest: add a method:

In `src/agent/store.rs`:
```rust
pub fn has_config_dir(&self) -> bool {
    self.config_dir.is_some()
}
```

Then in server.rs use `state.store.has_config_dir()`.

- [ ] **Step 6: Update server.rs test — use temp config dir to prevent HTTP download**

The existing test passes `None` for `config_dir`, which would trigger an HTTP download and fail. Change to use a temp config dir:

```rust
#[tokio::test]
async fn dispatch_task_persists_before_accepting() {
    let dir = tempdir().unwrap();
    // Create a minimal config dir so the handler skips HTTP download
    let cfg_dir = dir.path().join("my_config");
    std::fs::create_dir_all(cfg_dir.join("rules")).unwrap();
    std::fs::write(cfg_dir.join("source.toml"), b"[source]").unwrap();

    let state = AgentState {
        store: AgentStore::new(dir.path().join("agent_data"), Some(cfg_dir), "http://127.0.0.1:18080/api".to_string()).unwrap(),
        runner: AgentRunner::new("agent_1".to_string(), "http://127.0.0.1:18080/api".to_string()),
    };
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
```

(Replace the entire test function, not just the state initialization.)

- [ ] **Step 7: Run all tests**

Run: `cargo test --lib`
Expected: all tests pass (existing + new)

- [ ] **Step 8: Commit**

```bash
git add src/agent/store.rs src/agent/server.rs
git commit -m "feat(config-dist): add ensure_config, core_api_base to AgentStore, async download flow"
```

---

### Task 3: Agent — add POST /api/configs/update endpoint for hot update (C)

**Files:**
- Modify: `src/agent/server.rs` (new handler, add route)
- Modify: `src/core_agent_api.rs` (add `ConfigUpdateRequest` struct)

**Interfaces:**
- Consumes: `AgentStore::ensure_config_async`, existing `CoreState`
- Produces: `POST /api/configs/update` endpoint on Agent

- [ ] **Step 1: Add ConfigUpdateRequest to core_agent_api.rs**

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigUpdateRequest {
    pub snapshot_id: String,
    pub content_hash: String,
}
```

- [ ] **Step 2: Write test for the endpoint**

In `src/agent/server.rs` test module:

```rust
#[tokio::test]
async fn config_update_endpoint_accepts_notification() {
    let dir = tempdir().unwrap();
    // Create a fake cached config so the async download doesn't fail
    let snap_dir = dir.path().join("agent_data/config_snapshots/v_test");
    std::fs::create_dir_all(snap_dir.join("rules")).unwrap();
    std::fs::write(snap_dir.join("source.toml"), b"[source]").unwrap();
    std::fs::write(snap_dir.join("mapping_dx.ini"), b"[m]").unwrap();
    std::fs::write(snap_dir.join("load.toml"), b"[l]").unwrap();
    std::fs::write(snap_dir.join("rules/a.json"), b"{}").unwrap();

    let state = AgentState {
        store: AgentStore::new(dir.path().join("agent_data"), None, "http://127.0.0.1:9/api".to_string()).unwrap(),
        runner: AgentRunner::new("agent_1".to_string(), "http://127.0.0.1:9/api".to_string()),
    };
    let app = router(state);
    let body = serde_json::json!({ "snapshot_id": "v_test", "content_hash": "sha256:000" });
    let response = app.oneshot(
        Request::builder().method("POST").uri("/api/configs/update")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib agent::server::tests::config_update_endpoint_accepts_notification -v`
Expected: compilation error (no route for `/api/configs/update`)

- [ ] **Step 4: Implement handler + route**

In `src/agent/server.rs`:

Add imports:
```rust
use crate::core_agent_api::ConfigUpdateRequest;
use axum::extract::State;
```

Add to `router`:
```rust
pub fn router(state: AgentState) -> Router {
    Router::new()
        .route("/api/tasks", post(dispatch_task))
        .route("/api/configs/update", post(update_config))
        .with_state(state)
}
```

Add handler:
```rust
async fn update_config(
    State(state): State<AgentState>,
    Json(request): Json<ConfigUpdateRequest>,
) -> Json<serde_json::Value> {
    let snapshot_id = request.snapshot_id.clone();
    tracing::info!("[agent-server] config update notification: {snapshot_id} hash={}", request.content_hash);

    tokio::spawn(async move {
        match state.store.ensure_config_async(&snapshot_id, &state.runner.http).await {
            Ok(path) => tracing::info!("[agent-server] config {snapshot_id} ready at {}", path.display()),
            Err(e) => tracing::warn!("[agent-server] config {snapshot_id} download failed: {e}"),
        }
    });

    Json(serde_json::json!({ "accepted": true }))
}
```

Important: The handler spawns a tokio task because `ensure_config_async` may take time to download. The HTTP response returns immediately.

- [ ] **Step 5: Run all tests**

Run: `cargo test --lib`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/agent/server.rs src/core_agent_api.rs
git commit -m "feat(config-dist): add POST /api/configs/update endpoint for hot update"
```

---

### Task 4: Build and smoke test

**Files:** None (verification only)

- [ ] **Step 1: Build all binaries**

Run: `cargo build --release 2>&1`
Expected: success

- [ ] **Step 2: Prepare test config zip**

```bash
mkdir -p /tmp/p2_test/rules
cat > /tmp/p2_test/source.toml << 'EOF'
[source]
type = "local"
EOF
cat > /tmp/p2_test/mapping_dx.ini << 'EOF'
[tableMapping]
EOF
cat > /tmp/p2_test/load.toml << 'EOF'
[clickhouse]
EOF
cat > /tmp/p2_test/rules/rule.json << 'EOF'
{"table_name":"TPD_A"}
EOF
(cd /tmp/p2_test && zip -r /tmp/p2_config.zip .)
```

- [ ] **Step 3: Start Core, upload config, activate**

```bash
# Terminal 1: Core
./target/release/core --db /tmp/p2_core.db --config-storage /tmp/p2_cs

# Upload
curl -X POST http://127.0.0.1:18080/api/config-snapshots/upload \
  -H "content-type: application/octet-stream" \
  --data-binary @/tmp/p2_config.zip

# Activate (use the snapshot_id from upload response)
curl -X POST http://127.0.0.1:18080/api/config-snapshots/v_20260703_120000/activate
```

- [ ] **Step 4: Start Agent, dispatch task**

```bash
# Terminal 2: Agent (no --config-dir, so it uses HTTP download)
./target/release/core --db /tmp/p2_agent.db --config-storage /tmp/p2_as

# Wait for Agent to register, then dispatch a task with the same config_snapshot_id
curl -X POST http://127.0.0.1:18080/api/tasks/dispatch \
  -H "content-type: application/json" \
  -d '{
    "task_id": "integ_test_1",
    "logical_task_key": "test:2026-07-03 12:00:00:v_test",
    "strategy_id": "test_integ",
    "config_snapshot_id": "v_20260703_120000",
    "scan_start_time": "2026-07-03 12:00:00",
    "collect_id": "integ_collect",
    "load_type": "clickhouse",
    "encoding": "UTF-8",
    "output_delimiter": "|",
    "timeout_seconds": 300,
    "callback_base_url": "http://127.0.0.1:18080/api"
  }'
```

Verify:
- Agent logs show "downloading config" → "unpacked config" message
- `agent_data/config_snapshots/v_20260703_120000/` has the config files
- Task runs successfully or appropriately fails if no PM data

- [ ] **Step 5: Verify hot update notification**

Observe Core logs after activate: should show "notified agent agent_local of config v_..."
Agent logs should show "config update notification" followed by "config ready"

- [ ] **Step 6: Clean up and commit**

```bash
rm -rf /tmp/p2_test /tmp/p2_cs /tmp/p2_core.db /tmp/p2_agent.db /tmp/p2_config.zip
```

```bash
git add -A && git commit -m "feat(config-dist): phase 2 verified integration"
```

---

### Task 5: Edge cases and hardening

**Files:**
- Modify: `src/agent/store.rs` (add path traversal protection on zip extract)

- [ ] **Step 1: Write test for path traversal protection**

In `src/agent/store.rs` test module:

```rust
#[test]
fn rejects_path_traversal_in_zip() {
    use std::io::Write;
    let dir = tempdir().unwrap();
    let store = AgentStore::new(dir.path().join("agent_data"), None, "http://core/api".to_string()).unwrap();

    // Create a zip with traversal entries
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buf);
    let opts = zip::write::FileOptions::default();
    zip.start_file("../evil.sh", opts).unwrap();
    zip.write_all(b"rm -rf /").unwrap();
    zip.finish().unwrap();

    let config_root = dir.path().join("agent_data/config_snapshots/v_bad");
    std::fs::create_dir_all(&config_root).unwrap();
    let result = store.unpack_zip(buf.into_inner(), &config_root);
    assert!(result.is_err());
    assert!(!dir.path().join("evil.sh").exists());
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test --lib agent::store::tests::rejects_path_traversal_in_zip -v`
Expected: test fails (no `unpack_zip` method)

- [ ] **Step 3: Add path traversal protection**

Extract the unpack logic into a method and add validation:

```rust
pub fn unpack_zip(&self, zip_data: Vec<u8>, dest: &Path) -> Result<()> {
    use std::io::Read;
    let reader = std::io::Cursor::new(&zip_data);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| anyhow::anyhow!("invalid zip: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let raw_name = file.name().trim_end_matches('/').to_string();
        // Path traversal check
        let clean_name = raw_name.replace('\\', "/");
        if clean_name.contains("..") || clean_name.starts_with('/') {
            anyhow::bail!("path traversal detected: {raw_name}");
        }
        if file.is_dir() {
            std::fs::create_dir_all(dest.join(&clean_name))?;
        } else {
            let target = dest.join(&clean_name);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;
            std::fs::write(&target, &content)?;
        }
    }
    Ok(())
}
```

Then refactor `ensure_config_async` to call `self.unpack_zip(...)` instead of inline extract.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib agent::store::tests`
Expected: all pass (including new traversal test)

- [ ] **Step 5: Commit**

```bash
git add src/agent/store.rs
git commit -m "fix(config-dist): add path traversal protection to zip extract"
```
