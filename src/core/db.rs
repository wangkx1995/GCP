use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::core_agent_api::{AgentRegisterRequest, ConfigSnapshotMeta, ConfigSnapshotResponse, OnlineAgent, ResultRow, TaskResultReport, TaskStatus};

pub struct CoreDb {
    conn: Connection,
}

impl CoreDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS agents (
                agent_id TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                host TEXT NOT NULL,
                port INTEGER NOT NULL,
                version TEXT NOT NULL,
                capabilities_json TEXT NOT NULL,
                status TEXT NOT NULL,
                registered_at TEXT NOT NULL,
                last_heartbeat_at TEXT
            );
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
            CREATE TABLE IF NOT EXISTS collect_tasks (
                task_id TEXT PRIMARY KEY,
                logical_task_key TEXT NOT NULL,
                strategy_id TEXT NOT NULL,
                config_snapshot_id TEXT NOT NULL,
                scan_start_time TEXT NOT NULL,
                collect_id TEXT NOT NULL,
                assigned_agent_id TEXT NOT NULL,
                attempt_no INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                accepted_at TEXT,
                started_at TEXT,
                last_progress_at TEXT,
                finished_at TEXT,
                error_code TEXT,
                error_message TEXT
            );
            CREATE TABLE IF NOT EXISTS collect_result_cells (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                strategy_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                config_snapshot_id TEXT NOT NULL,
                table_name TEXT NOT NULL,
                data_time TEXT NOT NULL,
                row_count INTEGER NOT NULL,
                success INTEGER NOT NULL,
                collect_time TEXT NOT NULL,
                status TEXT NOT NULL,
                error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_collect_result_day ON collect_result_cells(strategy_id, data_time, table_name);
            "#,
        )?;
        let _ = self.conn.execute("ALTER TABLE config_snapshots ADD COLUMN version_label TEXT", []);
        let _ = self.conn.execute("ALTER TABLE config_snapshots ADD COLUMN is_active INTEGER NOT NULL DEFAULT 0", []);
        let _ = self.conn.execute("ALTER TABLE config_snapshots ADD COLUMN file_count INTEGER NOT NULL DEFAULT 0", []);
        let _ = self.conn.execute("ALTER TABLE config_snapshots ADD COLUMN activated_at TEXT", []);
        Ok(())
    }

    pub fn register_agent(&self, request: &AgentRegisterRequest) -> Result<String> {
        let agent_id = request.agent_id.clone().unwrap_or_else(|| format!("agent_{}", uuid::Uuid::new_v4().simple()));
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let capabilities_json = serde_json::to_string(&request.capabilities)?;
        self.conn.execute(
            r#"
            INSERT INTO agents(agent_id, agent_name, host, port, version, capabilities_json, status, registered_at, last_heartbeat_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'ONLINE', ?7, ?7)
            ON CONFLICT(agent_id) DO UPDATE SET
                agent_name=excluded.agent_name,
                host=excluded.host,
                port=excluded.port,
                version=excluded.version,
                capabilities_json=excluded.capabilities_json,
                status='ONLINE',
                last_heartbeat_at=excluded.last_heartbeat_at
            "#,
            rusqlite::params![agent_id, request.agent_name, request.host, request.port, request.version, capabilities_json, now],
        )?;
        Ok(agent_id)
    }

    pub fn update_agent_heartbeat(&self, agent_id: &str) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.conn.execute(
            "UPDATE agents SET status = 'ONLINE', last_heartbeat_at = ?2 WHERE agent_id = ?1",
            rusqlite::params![agent_id, now],
        )?;
        Ok(())
    }

    pub fn select_online_agent(&self) -> Result<(String, String, u16)> {
        // TODO: prefer agent with fewest running tasks instead of latest heartbeat
        // For v1, simple heartbeat-based selection is sufficient.
        let row = self.conn.query_row(
            "SELECT agent_id, host, port FROM agents WHERE status = 'ONLINE' ORDER BY last_heartbeat_at DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(row)
    }

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

    pub fn insert_config_snapshot(&self, snapshot: &ConfigSnapshotResponse) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.conn.execute(
            "INSERT OR REPLACE INTO config_snapshots(config_snapshot_id, content_hash, snapshot_json, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![snapshot.config_snapshot_id, snapshot.content_hash, serde_json::to_string(snapshot)?, now],
        )?;
        Ok(())
    }

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
            "SELECT config_snapshot_id, content_hash, version_label, is_active, file_count, created_at, activated_at FROM config_snapshots ORDER BY created_at DESC, config_snapshot_id DESC"
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
        self.conn.execute("UPDATE config_snapshots SET is_active = 0", [])?;
        self.conn.execute(
            "UPDATE config_snapshots SET is_active = 1, activated_at = ?2 WHERE config_snapshot_id = ?1",
            rusqlite::params![snapshot_id, now],
        )?;
        self.get_config_snapshot(snapshot_id)
    }

    pub fn create_task(&self, task_id: &str, logical_task_key: &str, strategy_id: &str, config_snapshot_id: &str, scan_start_time: &str, collect_id: &str, assigned_agent_id: &str) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.conn.execute(
            "INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'CREATED', ?8)",
            rusqlite::params![task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, now],
        )?;
        Ok(())
    }

    pub fn accept_task_result(&self, report: &TaskResultReport) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let task_row: Result<(String, String, String), _> = self.conn.query_row(
            "SELECT strategy_id, config_snapshot_id, status FROM collect_tasks WHERE task_id = ?1",
            rusqlite::params![report.task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        );

        let (strategy_id, config_snapshot_id) = match task_row {
            Ok((sid, cid, status)) => {
                tracing::info!("[core-db] accept_task_result: existing task status={status} strategy={sid}");
                match status.as_str() {
                    "SUCCEEDED" | "FAILED" | "TIMEOUT" | "CANCELLED" => {
                        anyhow::bail!("task {} is already in terminal state {}", report.task_id, status);
                    }
                    _ => {}
                }
                (sid, cid)
            }
            Err(_) => {
                tracing::info!("[core-db] accept_task_result: task not found, creating implicit record");
                let sid = format!("unknown_{}", report.task_id);
                let cid = "unknown".to_string();
                self.conn.execute(
                    "INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'CREATED', ?8)",
                    rusqlite::params![report.task_id, "", &sid, &cid, "", "", report.agent_id, now],
                )?;
                (sid, cid)
            }
        };

        let terminal_status = match report.status {
            TaskStatus::Succeeded => "SUCCEEDED",
            TaskStatus::Failed => "FAILED",
            TaskStatus::Timeout => "TIMEOUT",
            TaskStatus::Cancelled => "CANCELLED",
            _ => "SUCCEEDED",
        };

        tracing::info!("[core-db] inserting {} result cells for task {} (strategy={})", report.result_rows.len(), report.task_id, strategy_id);
        for result in &report.result_rows {
            tracing::debug!("[core-db]   cell: table={} data_time={} rows={} success={}", result.table_name, result.data_time, result.row_count, result.success);
            self.conn.execute(
                "INSERT INTO collect_result_cells(task_id, strategy_id, agent_id, config_snapshot_id, table_name, data_time, row_count, success, collect_time, status, error_message, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'SUCCEEDED', NULL, ?10, ?10)",
                rusqlite::params![report.task_id, strategy_id, report.agent_id, config_snapshot_id, result.table_name, result.data_time, result.row_count, result.success, result.collect_time, now],
            )?;
        }

        self.conn.execute(
            "UPDATE collect_tasks SET status = ?2, finished_at = ?3 WHERE task_id = ?1",
            rusqlite::params![report.task_id, terminal_status, now],
        )?;
        tracing::info!("[core-db] accept_task_result done: status={terminal_status}");
        Ok(())
    }

    pub fn result_rows_for_day(&self, strategy_id: &str, day: &str) -> Result<Vec<ResultRow>> {
        let like = format!("{day}%");
        let mut stmt = self.conn.prepare(
            "SELECT table_name, data_time, row_count, success, collect_time FROM collect_result_cells WHERE strategy_id = ?1 AND data_time LIKE ?2 ORDER BY table_name, data_time",
        )?;
        let rows = stmt.query_map(rusqlite::params![strategy_id, like], |row| {
            Ok(ResultRow {
                table_name: row.get(0)?,
                data_time: row.get(1)?,
                row_count: row.get::<_, i64>(2)? as u64,
                success: row.get(3)?,
                collect_time: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use super::*;
    use crate::core_agent_api::{AgentCapabilities, RuleFile, TaskStatus};
    use tempfile::{tempdir, TempDir};

    struct TestDb {
        db: CoreDb,
        _dir: TempDir,
    }

    impl Deref for TestDb {
        type Target = CoreDb;
        fn deref(&self) -> &CoreDb {
            &self.db
        }
    }

    fn db() -> TestDb {
        let dir = tempdir().unwrap();
        let db = CoreDb::open(dir.path().join("core.db")).unwrap();
        TestDb { db, _dir: dir }
    }

    fn agent_request() -> AgentRegisterRequest {
        AgentRegisterRequest {
            agent_id: None,
            agent_name: "agent-1".to_string(),
            host: "127.0.0.1".to_string(),
            port: 18081,
            version: "1.0.0".to_string(),
            capabilities: AgentCapabilities {
                can_collect: true,
                can_parse: true,
                can_load: false,
                supported_protocols: vec!["ftp".to_string(), "sftp".to_string()],
            },
        }
    }

    #[test]
    fn registers_agent_and_reuses_existing_agent_id() {
        let db = db();
        let agent_id = db.register_agent(&agent_request()).unwrap();
        let mut reconnect = agent_request();
        reconnect.agent_id = Some(agent_id.clone());
        let reused = db.register_agent(&reconnect).unwrap();
        assert_eq!(reused, agent_id);
    }

    #[test]
    fn stores_task_result_rows() {
        let db = db();
        let agent_id = db.register_agent(&agent_request()).unwrap();
        db.insert_config_snapshot(&ConfigSnapshotResponse {
            config_snapshot_id: "cfg_1".to_string(),
            content_hash: "sha256:test".to_string(),
            source_toml: "[source]".to_string(),
            mapping_dx_ini: "[m]".to_string(),
            load_toml: "[load]".to_string(),
            col_name_cut_config_ini: None,
            rules: vec![RuleFile { relative_path: "rules/a.json".to_string(), content: "{\"table_name\":\"TPD_A\"}".to_string() }],
        }).unwrap();
        db.create_task("task_1", "strategy_1:2026-06-17 15:15:00:cfg_1", "strategy_1", "cfg_1", "2026-06-17 15:15:00", "collect_1", &agent_id).unwrap();
        db.accept_task_result(&TaskResultReport {
            task_id: "task_1".to_string(),
            agent_id,
            status: TaskStatus::Succeeded,
            result_rows: vec![ResultRow {
                table_name: "TPD_A".to_string(),
                data_time: "2026-06-17 15:15:00".to_string(),
                row_count: 123,
                success: 1,
                collect_time: "2026-07-02 15:35:00".to_string(),
            }],
        }).unwrap();
        let rows = db.result_rows_for_day("strategy_1", "2026-06-17").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].table_name, "TPD_A");
        assert_eq!(rows[0].row_count, 123);
        let stored_status: String = db.conn.query_row(
            "SELECT status FROM collect_tasks WHERE task_id = 'task_1'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(stored_status, "SUCCEEDED");
    }

    #[test]
    fn inserts_and_lists_config_snapshots() {
        let db = db();
        db.insert_config_snapshot_meta("v1", "sha256:aaa", "v1_label", 5).unwrap();
        db.insert_config_snapshot_meta("v2", "sha256:bbb", "v2_label", 3).unwrap();

        let list = db.list_config_snapshots().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].config_snapshot_id, "v2");
    }

    #[test]
    fn lists_online_agents() {
        let db = db();
        let mut req = agent_request();
        req.agent_id = Some("agent_a".into());
        db.register_agent(&req).unwrap();

        let agents = db.list_online_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "agent_a");
        assert_eq!(agents[0].host, "127.0.0.1");
        assert_eq!(agents[0].port, 18081);
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
}
