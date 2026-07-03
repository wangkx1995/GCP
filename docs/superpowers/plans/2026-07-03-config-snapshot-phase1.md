# Config Snapshot Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Core-side config snapshot upload, validation, version management, and activate/rollback. No Agent SFTP distribution (Phase 2) or web UI (Phase 3).

**Architecture:** New `ConfigStorage` module handles zip validation and filesystem layout. CoreDB gets new columns and CRUD for config version metadata. HTTP handlers wire everything together. Existing `GET /api/config-snapshots/{id}` stub is replaced.

**Tech Stack:** Rust, axum, rusqlite, zip crate, sha2 crate

## Global Constraints

- All config snapshot data on disk at configurable `--config-storage` path
- DB schema changes are additive (add columns, don't remove)
- Existing `config_snapshot` DB migration: CREATE TABLE IF NOT EXISTS already handles schema; new columns added via ALTER TABLE IF NOT EXISTS pattern
- Content hash = sha256 of sorted `(path\0content\0)` pairs (deterministic)
- Zip validation checks for source.toml, mapping_dx.ini, load.toml, rules/ dir
- Error responses return structured JSON with error list

---

### Task 1: Add sha2 dep, create ConfigStorage skeleton, add --config-storage CLI arg

**Files:**
- Modify: `Cargo.toml` (add sha2 dep)
- Create: `src/core/config_storage.rs` (skeleton with struct, path helpers)
- Modify: `src/core/mod.rs` (add `pub mod config_storage;`)
- Modify: `src/bin/core.rs` (add `--config-storage` arg)

- [ ] **Step 1: Add sha2 to Cargo.toml**

```toml
sha2 = "0.10"
```

- [ ] **Step 2: Create `src/core/config_storage.rs` skeleton**

```rust
use std::path::{Path, PathBuf};
use anyhow::Result;

#[derive(Debug)]
pub struct ValidationError {
    pub errors: Vec<String>,
}

#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub config_snapshot_id: String,
    pub content_hash: String,
    pub file_count: usize,
}

#[derive(Clone, Debug)]
pub struct ConfigStorage {
    root: PathBuf,
}

impl ConfigStorage {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("versions"))?;
        let storage = Self { root };
        Ok(storage)
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn active_link(&self) -> PathBuf {
        self.root.join("active")
    }

    pub fn version_dir(&self, snapshot_id: &str) -> PathBuf {
        self.versions_dir().join(snapshot_id)
    }

    pub fn validate_and_unpack(&self, zip_data: &[u8], snapshot_id: &str) -> Result<ValidationResult> {
        // TODO: Task 2
        anyhow::bail!("not implemented yet")
    }
}
```

- [ ] **Step 3: Update `src/core/mod.rs`**

```rust
pub mod config_storage;
pub mod db;
pub mod grid;
pub mod server;
```

- [ ] **Step 4: Update `src/bin/core.rs` to add `--config-storage`**

```rust
#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:18080")]
    listen: SocketAddr,
    #[arg(long, default_value = "core.db")]
    db: PathBuf,
    #[arg(long, default_value = "config_storage")]
    config_storage: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // ... existing setup ...
    let cli = Cli::parse();
    let config_storage = crate::core::config_storage::ConfigStorage::new(cli.config_storage)?;
    tracing::info!("[core] starting listen={} db={} config_storage={:?}", cli.listen, cli.db.display(), config_storage.versions_dir());
    let result = crate::core::server::run_core_server(cli.listen, cli.db, config_storage).await;
    result
}
```

- [ ] **Step 5: Run tests to verify no regressions**

Run: `cargo test --lib`
Expected: all 34 tests pass

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/core/config_storage.rs src/core/mod.rs src/bin/core.rs
git commit -m "feat(config-snapshot): add sha2 dep, ConfigStorage skeleton, --config-storage CLI arg"
```

---

### Task 2: Implement ConfigStorage zip validation and unpack

**Files:**
- Modify: `src/core/config_storage.rs` (full implementation)

- [ ] **Step 1: Write tests for zip validation**

Add inside `#[cfg(test)] mod tests { }` at the bottom of `config_storage.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn make_valid_zip() -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::FileOptions::default();
        zip.add_directory("rules/", options).unwrap();
        zip.start_file("source.toml", options).unwrap();
        zip.write_all(b"[source]\ntype=\"sftp\"").unwrap();
        zip.start_file("mapping_dx.ini", options).unwrap();
        zip.write_all(b"[tableMapping]").unwrap();
        zip.start_file("load.toml", options).unwrap();
        zip.write_all(b"[clickhouse]").unwrap();
        zip.start_file("rules/rule_a.json", options).unwrap();
        zip.write_all(b"{\"table_name\":\"TPD_A\"}").unwrap();
        zip.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn validates_valid_zip() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let zip_data = make_valid_zip();
        let result = storage.validate_and_unpack(&zip_data, "v1_test").unwrap();
        assert!(result.valid);
        assert!(result.errors.is_empty());
        assert_eq!(result.file_count, 5);
        assert!(!result.content_hash.is_empty());
    }

    #[test]
    fn rejects_zip_missing_source_toml() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::FileOptions::default();
        zip.add_directory("rules/", options).unwrap();
        zip.start_file("mapping_dx.ini", options).unwrap();
        zip.write_all(b"[tableMapping]").unwrap();
        zip.finish().unwrap();
        let result = storage.validate_and_unpack(buf.into_inner(), "v2_test").unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("source.toml")));
    }

    #[test]
    fn rejects_zip_missing_rules_dir() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::FileOptions::default();
        zip.start_file("source.toml", options).unwrap();
        zip.write_all(b"[source]").unwrap();
        zip.start_file("mapping_dx.ini", options).unwrap();
        zip.write_all(b"[tableMapping]").unwrap();
        zip.start_file("load.toml", options).unwrap();
        zip.write_all(b"[clickhouse]").unwrap();
        zip.finish().unwrap();
        let result = storage.validate_and_unpack(buf.into_inner(), "v3_test").unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("rules")));
    }

    #[test]
    fn unpack_writes_files_to_disk() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let zip_data = make_valid_zip();
        let result = storage.validate_and_unpack(&zip_data, "v1_test").unwrap();
        assert!(result.valid);
        let vdir = storage.version_dir("v1_test");
        assert!(vdir.join("source.toml").exists());
        assert!(vdir.join("mapping_dx.ini").exists());
        assert!(vdir.join("load.toml").exists());
        assert!(vdir.join("rules").join("rule_a.json").exists());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config_storage::tests -v 2>&1 | head -30`
Expected: compilation errors or test failures

- [ ] **Step 3: Implement `validate_and_unpack`**

Replace the stub with full implementation in `src/core/config_storage.rs`:

```rust
use std::io::Read;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tracing::info;

const REQUIRED_FILES: &[&str] = &["source.toml", "mapping_dx.ini", "load.toml"];

#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub config_snapshot_id: String,
    pub content_hash: String,
    pub file_count: usize,
}

#[derive(Clone, Debug)]
pub struct ConfigStorage {
    root: PathBuf,
}

impl ConfigStorage {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("versions"))?;
        info!("[config-storage] root={}", root.display());
        Ok(Self { root })
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn active_link(&self) -> PathBuf {
        self.root.join("active")
    }

    pub fn version_dir(&self, snapshot_id: &str) -> PathBuf {
        self.versions_dir().join(snapshot_id)
    }

    pub fn validate_and_unpack(&self, zip_data: &[u8], snapshot_id: &str) -> Result<ValidationResult> {
        let mut errors: Vec<String> = Vec::new();
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        let mut has_rules_dir = false;

        let reader = std::io::Cursor::new(zip_data);
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| anyhow::anyhow!("invalid zip: {e}"))?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().trim_end_matches('/').to_string();
            if file.is_dir() {
                if name == "rules" || name.starts_with("rules/") {
                    has_rules_dir = true;
                }
                continue;
            }
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;
            entries.push((name, content));
        }

        let entry_names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();

        for required in REQUIRED_FILES {
            if !entry_names.contains(required) {
                errors.push(format!("missing required file: {required}"));
            }
        }
        if !has_rules_dir {
            let has_rule_file = entry_names.iter().any(|n| n.starts_with("rules/"));
            if !has_rule_file {
                errors.push("missing required directory: rules/".to_string());
            }
        }

        if !errors.is_empty() {
            return Ok(ValidationResult {
                valid: false,
                errors,
                config_snapshot_id: snapshot_id.to_string(),
                content_hash: String::new(),
                file_count: entries.len(),
            });
        }

        // Sort entries by relative_path for deterministic hashing
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        // Compute hash
        let mut hasher = Sha256::new();
        for (name, content) in &entries {
            hasher.update(name.as_bytes());
            hasher.update(b"\0");
            hasher.update(content);
            hasher.update(b"\0");
        }
        let hash = format!("sha256:{}", hex::encode(hasher.finalize()));

        // Write to disk
        let version_dir = self.version_dir(snapshot_id);
        std::fs::create_dir_all(&version_dir)
            .with_context(|| format!("create version dir {}", version_dir.display()))?;

        for (name, content) in &entries {
            let target = version_dir.join(name);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create parent dir {}", parent.display()))?;
            }
            std::fs::write(&target, content)
                .with_context(|| format!("write {}", target.display()))?;
        }

        info!("[config-storage] unpacked {} files to {}", entries.len(), version_dir.display());

        Ok(ValidationResult {
            valid: true,
            errors: Vec::new(),
            config_snapshot_id: snapshot_id.to_string(),
            content_hash: hash,
            file_count: entries.len(),
        })
    }

    pub fn delete_version(&self, snapshot_id: &str) -> Result<()> {
        let vdir = self.version_dir(snapshot_id);
        if vdir.exists() {
            std::fs::remove_dir_all(&vdir)?;
        }
        Ok(())
    }
}
```

Note: Add `use hex` or handle hex encoding. `hex` crate may need to be added. Let me check if it's already available.

Actually, let me use `format!("{:x}", sha2::Digest::finalize(...))` or add hex crate. The simplest approach: format hex manually.

```rust
let hash_value = hasher.finalize();
let hash = format!("sha256:{}", hash_value.iter().map(|b| format!("{b:02x}")).collect::<String>());
```

This avoids adding a hex dep.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config_storage::tests -v`
Expected: all 4 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/core/config_storage.rs
git commit -m "feat(config-snapshot): implement zip validation, hash, and disk unpack"
```

---

### Task 3: Update Core DB schema and CRUD methods for config snapshot metadata

**Files:**
- Modify: `src/core/db.rs` (new columns, CRUD methods)

- [ ] **Step 1: Write tests for new DB methods**

Add to existing test module in `src/core/db.rs`. Note: `hex` encoding needed for the fake content_hash; use string literals.

```rust
#[test]
fn inserts_and_lists_config_snapshots() {
    let db = db();
    db.insert_config_snapshot_meta("v1", "sha256:aaa", "v1_label", 5).unwrap();
    db.insert_config_snapshot_meta("v2", "sha256:bbb", "v2_label", 3).unwrap();

    let list = db.list_config_snapshots().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].config_snapshot_id, "v2"); // most recent first
}

#[test]
fn activate_switches_snapshot_is_active() {
    let db = db();
    db.insert_config_snapshot_meta("v1", "sha256:aaa", "v1_label", 5).unwrap();
    db.insert_config_snapshot_meta("v2", "sha256:bbb", "v2_label", 3).unwrap();

    let meta = db.activate_config_snapshot("v1").unwrap();
    assert!(meta.is_active);

    let v1 = db.get_config_snapshot("v1").unwrap();
    assert!(v1.is_active);
    let v2 = db.get_config_snapshot("v2").unwrap();
    assert!(!v2.is_active);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib db::tests::inserts_and_lists_config_snapshots -v`
Expected: compilation error (method doesn't exist)

- [ ] **Step 3: Add new columns to `init_schema` + implement CRUD methods**

Update `init_schema` in `src/core/db.rs`. The SQLite `CREATE TABLE IF NOT EXISTS` won't add new columns to existing tables. Use `ALTER TABLE IF NOT EXISTS` pattern or just use `CREATE TABLE IF NOT EXISTS` with the new schema (since the existing table might not have the new columns, but CREATE TABLE IF NOT EXISTS only creates if the table doesn't exist at all).

For MVP simplicity: since this is v0 and the table might already exist without new columns, use a migration approach:

```rust
fn init_schema(&self) -> Result<()> {
    self.conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS config_snapshots (
            config_snapshot_id TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            version_label TEXT,
            is_active INTEGER NOT NULL DEFAULT 0,
            file_count INTEGER NOT NULL DEFAULT 0,
            snapshot_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            activated_at TEXT
        );
        -- existing other tables...
        "#,
    )?;
    // Migrate: add columns if they don't exist (ignore errors for existing cols)
    let _ = self.conn.execute("ALTER TABLE config_snapshots ADD COLUMN version_label TEXT", []);
    let _ = self.conn.execute("ALTER TABLE config_snapshots ADD COLUMN is_active INTEGER NOT NULL DEFAULT 0", []);
    let _ = self.conn.execute("ALTER TABLE config_snapshots ADD COLUMN file_count INTEGER NOT NULL DEFAULT 0", []);
    let _ = self.conn.execute("ALTER TABLE config_snapshots ADD COLUMN activated_at TEXT", []);
    Ok(())
}
```

Add CRUD methods:

```rust
pub fn insert_config_snapshot_meta(&self, snapshot_id: &str, content_hash: &str, version_label: &str, file_count: usize) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    self.conn.execute(
        "INSERT OR REPLACE INTO config_snapshots(config_snapshot_id, content_hash, version_label, is_active, file_count, snapshot_json, created_at, activated_at) VALUES (?1, ?2, ?3, 0, ?4, '{}', ?5, NULL)",
        rusqlite::params![snapshot_id, content_hash, version_label, file_count, now],
    )?;
    Ok(())
}

pub fn list_config_snapshots(&self) -> Result<Vec<ConfigSnapshotMeta>> {
    let mut stmt = self.conn.prepare(
        "SELECT config_snapshot_id, content_hash, version_label, is_active, file_count, created_at, activated_at FROM config_snapshots ORDER BY created_at DESC"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ConfigSnapshotMeta {
            config_snapshot_id: row.get(0)?,
            content_hash: row.get(1)?,
            version_label: row.get::<_, Option<String>>(2)?,
            is_active: row.get::<_, i32>(3)? != 0,
            file_count: row.get::<_, i32>(4)? as usize,
            created_at: row.get(5)?,
            activated_at: row.get::<_, Option<String>>(6)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

pub fn get_config_snapshot(&self, snapshot_id: &str) -> Result<ConfigSnapshotMeta> {
    self.conn.query_row(
        "SELECT config_snapshot_id, content_hash, version_label, is_active, file_count, created_at, activated_at FROM config_snapshots WHERE config_snapshot_id = ?1",
        rusqlite::params![snapshot_id],
        |row| {
            Ok(ConfigSnapshotMeta {
                config_snapshot_id: row.get(0)?,
                content_hash: row.get(1)?,
                version_label: row.get::<_, Option<String>>(2)?,
                is_active: row.get::<_, i32>(3)? != 0,
                file_count: row.get::<_, i32>(4)? as usize,
                created_at: row.get(5)?,
                activated_at: row.get::<_, Option<String>>(6)?,
            })
        },
    ).map_err(Into::into)
}

pub fn activate_config_snapshot(&self, snapshot_id: &str) -> Result<ConfigSnapshotMeta> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    // Deactivate all
    self.conn.execute("UPDATE config_snapshots SET is_active = 0", [])?;
    // Activate target
    self.conn.execute(
        "UPDATE config_snapshots SET is_active = 1, activated_at = ?2 WHERE config_snapshot_id = ?1",
        rusqlite::params![snapshot_id, now],
    )?;
    self.get_config_snapshot(snapshot_id)
}
```

Add the `ConfigSnapshotMeta` struct near the top of `db.rs` or in `core_agent_api.rs`. For simplicity, add to core_agent_api.rs:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigSnapshotMeta {
    pub config_snapshot_id: String,
    pub content_hash: String,
    pub version_label: Option<String>,
    pub is_active: bool,
    pub file_count: usize,
    pub created_at: String,
    pub activated_at: Option<String>,
}
```

- [ ] **Step 4: Run all tests to verify**

Run: `cargo test --lib`
Expected: all tests pass (new + old 36 tests)

- [ ] **Step 5: Commit**

```bash
git add src/core/db.rs src/core_agent_api.rs
git commit -m "feat(config-snapshot): add config snapshot CRUD, activate, DB migration"
```

---

### Task 4: Implement Core HTTP endpoints for config snapshot management

**Files:**
- Modify: `src/core/server.rs` (new handlers, update state, update routes)
- Modify: `src/bin/core.rs` (pass ConfigStorage to server)

- [ ] **Step 1: Update `run_core_server` to accept ConfigStorage**

Update `src/core/server.rs`:

```rust
use crate::core::config_storage::ConfigStorage;

#[derive(Clone)]
pub struct CoreState {
    pub db: Arc<Mutex<CoreDb>>,
    pub http: reqwest::Client,
    pub storage: Arc<ConfigStorage>,
}

pub async fn run_core_server(addr: SocketAddr, db_path: PathBuf, storage: ConfigStorage) -> Result<()> {
    let state = CoreState {
        db: Arc::new(Mutex::new(CoreDb::open(db_path)?)),
        http: reqwest::Client::new(),
        storage: Arc::new(storage),
    };
    // ... existing setup ...
}
```

- [ ] **Step 2: Update route table and replace stub**

Replace the old stub route with new endpoints:

```rust
pub fn router(state: CoreState) -> Router {
    Router::new()
        .route("/api/agents/register", post(register_agent))
        .route("/api/agents/:agent_id/heartbeat", post(heartbeat))
        .route("/api/config-snapshots/upload", post(upload_config_snapshot))
        .route("/api/config-snapshots", get(list_config_snapshots))
        .route("/api/config-snapshots/{id}/activate", post(activate_config_snapshot))
        .route("/api/config-snapshots/{id}/download", get(download_config_snapshot))
        .route("/api/config-snapshots/{id}", get(get_config_snapshot))
        .route("/api/tasks/:task_id/events", post(task_event))
        .route("/api/tasks/:task_id/result", post(task_result))
        .route("/api/tasks/dispatch", post(dispatch_task))
        .route("/api/results/grid", get(result_grid))
        .with_state(state)
}
```

Note: Route ordering matters for axum. Static paths like `/upload` must come before parameterized paths like `/{id}`. `/api/config-snapshots` (list) and `/api/config-snapshots/upload` must come before `/{id}` routes. Axum's router should handle this correctly with recent versions.

- [ ] **Step 3: Implement handlers**

Remove the old `config_snapshot` stub handler and add:

```rust
async fn upload_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if body.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty request body".to_string()));
    }

    let snapshot_id = format!("v_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"));
    let result = state.storage.validate_and_unpack(&body, &snapshot_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("storage error: {e}")))?;

    if !result.valid {
        return Err((StatusCode::BAD_REQUEST, serde_json::json!({
            "valid": false,
            "errors": result.errors,
            "config_snapshot_id": snapshot_id,
        }).to_string()));
    }

    state.db.lock().await.insert_config_snapshot_meta(&snapshot_id, &result.content_hash, &snapshot_id, result.file_count)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    tracing::info!("[core] uploaded config snapshot {snapshot_id} ({} files, hash={})", result.file_count, result.content_hash);
    Ok(Json(serde_json::json!({
        "valid": true,
        "config_snapshot_id": snapshot_id,
        "content_hash": result.content_hash,
        "file_count": result.file_count,
    })))
}

async fn list_config_snapshots(
    axum::extract::State(state): axum::extract::State<CoreState>,
) -> Result<Json<Vec<ConfigSnapshotMeta>>, (StatusCode, String)> {
    let list = state.db.lock().await.list_config_snapshots()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
    Ok(Json(list))
}

async fn get_config_snapshot_handler(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ConfigSnapshotMeta>, (StatusCode, String)> {
    let meta = state.db.lock().await.get_config_snapshot(&id)
        .map_err(|e| {
            let msg = format!("{e:#}");
            if msg.contains("QueryReturnedNoRows") {
                (StatusCode::NOT_FOUND, format!("snapshot {id} not found"))
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
            }
        })?;
    Ok(Json(meta))
}

async fn activate_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Check snapshot exists
    let meta = state.db.lock().await.get_config_snapshot(&id)
        .map_err(|_| (StatusCode::NOT_FOUND, format!("snapshot {id} not found")))?;

    // Atomically swap symlink
    let target = state.storage.version_dir(&id);
    if !target.exists() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("version dir {} missing", target.display())));
    }

    let active = state.storage.active_link();
    let temp = active.with_extension("tmp");
    // Create symlink (replace if exists)
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&temp);
        std::os::unix::fs::symlink(&target, &temp).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("symlink error: {e}")))?;
        std::fs::rename(&temp, &active).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("rename error: {e}")))?;
    }
    #[cfg(not(unix))]
    {
        // Fallback: just write the target path
        std::fs::write(&active, target.to_string_lossy().as_bytes())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write error: {e}")))?;
    }

    let meta = state.db.lock().await.activate_config_snapshot(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    tracing::info!("[core] activated config snapshot {id}");

    Ok(Json(serde_json::json!({
        "config_snapshot_id": id,
        "active": true,
        "content_hash": meta.content_hash,
        "activated_at": meta.activated_at,
    })))
}

async fn download_config_snapshot(
    axum::extract::State(state): axum::extract::State<CoreState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let vdir = state.storage.version_dir(&id);
    if !vdir.exists() {
        return Err((StatusCode::NOT_FOUND, format!("snapshot {id} not found on disk")));
    }
    let zip_data = crate::core::config_storage::create_zip_from_dir(&vdir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("zip error: {e}")))?;
    Ok(axum::response::Response::builder()
        .header("content-type", "application/zip")
        .header("content-disposition", format!("attachment; filename=\"{id}.zip\""))
        .body(axum::body::Body::from(zip_data))
        .unwrap())
}
```

- [ ] **Step 4: Add `create_zip_from_dir` helper to config_storage.rs**

```rust
pub fn create_zip_from_dir(dir: &Path) -> Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buf);
    let options = zip::write::FileOptions::default();

    let entries = collect_files(dir, dir);
    for (rel_path, content) in &entries {
        let name = rel_path.to_string_lossy().replace('\\', "/");
        if content.is_empty() && name.ends_with('/') {
            zip.add_directory(&name, options).unwrap();
        } else {
            zip.start_file(&name, options).unwrap();
            use std::io::Write;
            zip.write_all(content).unwrap();
        }
    }
    zip.finish()?;
    Ok(buf.into_inner())
}

fn collect_files(base: &Path, dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap().to_path_buf();
            if path.is_dir() {
                result.push((rel.join(""), Vec::new())); // dir marker
                result.extend(collect_files(base, &path));
            } else if path.is_file() {
                if let Ok(content) = std::fs::read(&path) {
                    result.push((rel, content));
                }
            }
        }
    }
    result
}
```

- [ ] **Step 5: Update test in server.rs to include storage field**

The existing test creates `CoreState` and needs to include `storage`:

```rust
let storage = ConfigStorage::new(dir.path().join("config_storage")).unwrap();
let state = CoreState {
    db: Arc::new(Mutex::new(CoreDb::open(dir.path().join("core.db")).unwrap())),
    http: reqwest::Client::new(),
    storage: Arc::new(storage),
};
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib`
Expected: all tests pass

- [ ] **Step 7: Commit**

```bash
git add src/core/server.rs src/bin/core.rs src/core/config_storage.rs
git commit -m "feat(config-snapshot): implement upload/list/get/activate/download HTTP endpoints"
```

---

### Task 5: Integration verification

- [ ] **Step 1: Build release**

Run: `cargo build --release --bin core`
Expected: success

- [ ] **Step 2: Manual smoke test - upload a config zip**

Create a test zip:

```bash
mkdir -p /tmp/test_zip/rules
cat > /tmp/test_zip/source.toml << 'EOF'
[source]
type = "sftp"
EOF
cat > /tmp/test_zip/mapping_dx.ini << 'EOF'
[tableMapping]
EOF
cat > /tmp/test_zip/load.toml << 'EOF'
[clickhouse]
EOF
cat > /tmp/test_zip/rules/rule.json << 'EOF'
{"table_name":"TPD_A"}
EOF
(cd /tmp/test_zip && zip -r /tmp/test_config.zip .)
```

Start Core:

```bash
./target/release/core --config-storage /tmp/test_cs --db /tmp/test_cs/core.db
```

Upload via curl (raw zip bytes, not multipart):

```bash
curl -X POST http://127.0.0.1:18080/api/config-snapshots/upload \
  -H "content-type: application/octet-stream" \
  --data-binary @/tmp/test_config.zip
```

Expected: 200 with `{"valid":true,"config_snapshot_id":"v_...","content_hash":"sha256:...","file_count":5}`

- [ ] **Step 3: Verify list, get, activate**

```bash
# List all snapshots
curl http://127.0.0.1:18080/api/config-snapshots

# Get metadata for the uploaded snapshot
curl http://127.0.0.1:18080/api/config-snapshots/v_20260703_120000

# Activate
curl -X POST http://127.0.0.1:18080/api/config-snapshots/v_20260703_120000/activate

# Verify active symlink
readlink /tmp/test_cs/active
```

- [ ] **Step 4: Verify validation error response**

```bash
# Create an invalid zip (missing source.toml)
mkdir -p /tmp/bad_zip/rules
cat > /tmp/bad_zip/load.toml << 'EOF'
[clickhouse]
EOF
(cd /tmp/bad_zip && zip -r /tmp/bad_config.zip .)

curl -X POST http://127.0.0.1:18080/api/config-snapshots/upload \
  -H "content-type: application/octet-stream" \
  --data-binary @/tmp/bad_config.zip
```

Expected: 400 with `{"valid":false,"errors":["missing required file: mapping_dx.ini","missing required file: source.toml"]}`

- [ ] **Step 5: Clean up and commit final adjustments**

```bash
rm -rf /tmp/test_cs /tmp/test_zip /tmp/test_config.zip /tmp/bad_zip /tmp/bad_config.zip
```

```bash
git add -A && git commit -m "feat(config-snapshot): phase 1 complete with integration verification"
```
