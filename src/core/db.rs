use std::path::Path;

use anyhow::Result;
use sqlx::Row;
use sqlx::SqlitePool;

use crate::core_agent_api::{
    AgentCapabilities, AgentInfo, AgentRegisterRequest, AgentStatus, ConfigSnapshotMeta,
    ConfigSnapshotResponse, OnlineAgent, ResultRow, TaskResultReport, TaskStatus,
};

#[derive(Clone)]
pub struct CoreDb {
    pool: SqlitePool,
}

impl CoreDb {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let url = format!("sqlite:{}?mode=rwc", path.as_ref().display());
        let pool = SqlitePool::connect(&url).await?;
        let db = Self { pool };
        db.init_schema().await?;
        Ok(db)
    }

    async fn init_schema(&self) -> Result<()> {
        sqlx::query(
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
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
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
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
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
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
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
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_collect_result_day ON collect_result_cells(strategy_id, data_time, table_name)"#,
        )
        .execute(&self.pool)
        .await
        .ok();

        let _ = sqlx::query("ALTER TABLE config_snapshots ADD COLUMN version_label TEXT")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query(
            "ALTER TABLE config_snapshots ADD COLUMN is_active INTEGER NOT NULL DEFAULT 0",
        )
        .execute(&self.pool)
        .await;
        let _ = sqlx::query(
            "ALTER TABLE config_snapshots ADD COLUMN file_count INTEGER NOT NULL DEFAULT 0",
        )
        .execute(&self.pool)
        .await;
        let _ = sqlx::query("ALTER TABLE config_snapshots ADD COLUMN activated_at TEXT")
            .execute(&self.pool)
        .await;
        Ok(())
    }

    pub async fn register_agent(&self, request: &AgentRegisterRequest) -> Result<String> {
        let agent_id = request
            .agent_id
            .clone()
            .unwrap_or_else(|| format!("agent_{}", uuid::Uuid::new_v4().simple()));
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let capabilities_json = serde_json::to_string(&request.capabilities)?;
        tracing::debug!(
            "[db] ==> INSERT INTO agents(agent_id,agent_name,host,port,version,capabilities_json,status,registered_at,last_heartbeat_at) VALUES(?,?,?,?,?,?,'ONLINE',?,?) ON CONFLICT(agent_id) DO UPDATE SET agent_name=excluded.agent_name,host=excluded.host,port=excluded.port,version=excluded.version,capabilities_json=excluded.capabilities_json,status='ONLINE',last_heartbeat_at=excluded.last_heartbeat_at"
        );
        tracing::debug!("[db] ==> Parameters: agent_id={}, agent_name={}, host={}, port={}, version={}", agent_id, request.agent_name, request.host, request.port, request.version);
        sqlx::query(
            r#"
            INSERT INTO agents(agent_id, agent_name, host, port, version, capabilities_json, status, registered_at, last_heartbeat_at)
            VALUES (?, ?, ?, ?, ?, ?, 'ONLINE', ?, ?)
            ON CONFLICT(agent_id) DO UPDATE SET
                agent_name=excluded.agent_name,
                host=excluded.host,
                port=excluded.port,
                version=excluded.version,
                capabilities_json=excluded.capabilities_json,
                status='ONLINE',
                last_heartbeat_at=excluded.last_heartbeat_at
            "#,
        )
        .bind(&agent_id)
        .bind(&request.agent_name)
        .bind(&request.host)
        .bind(request.port)
        .bind(&request.version)
        .bind(&capabilities_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(agent_id)
    }

    pub async fn update_agent_heartbeat(&self, agent_id: &str) -> Result<()> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        tracing::debug!("[db] ==> UPDATE agents SET status='ONLINE',last_heartbeat_at=? WHERE agent_id=?");
        tracing::debug!("[db] ==> Parameters: last_heartbeat_at={}, agent_id={}", now, agent_id);
        sqlx::query(
            "UPDATE agents SET status = 'ONLINE', last_heartbeat_at = ? WHERE agent_id = ?",
        )
        .bind(&now)
        .bind(agent_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_stale_agents_offline(&self, max_age_seconds: i64) -> Result<usize> {
        let cutoff = (chrono::Local::now() - chrono::Duration::seconds(max_age_seconds))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        tracing::debug!("[db] ==> UPDATE agents SET status='OFFLINE' WHERE status='ONLINE' AND last_heartbeat_at<?");
        tracing::debug!("[db] ==> Parameters: cutoff={}", cutoff);
        let result = sqlx::query(
            "UPDATE agents SET status = 'OFFLINE' WHERE status = 'ONLINE' AND last_heartbeat_at < ?",
        )
        .bind(&cutoff)
        .execute(&self.pool)
        .await?;
        let n = result.rows_affected() as usize;
        Ok(n)
    }

    pub async fn select_online_agent(&self) -> Result<(String, String, u16)> {
        tracing::debug!("[db] ==> SELECT agent_id,host,port FROM agents WHERE status='ONLINE' ORDER BY last_heartbeat_at DESC LIMIT 1");
        let row = sqlx::query(
            "SELECT agent_id, host, port FROM agents WHERE status = 'ONLINE' ORDER BY last_heartbeat_at DESC LIMIT 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((row.get(0), row.get(1), row.get(2)))
    }

    pub async fn list_all_agents(&self) -> Result<Vec<AgentInfo>> {
        tracing::debug!("[db] ==> SELECT agent_id,agent_name,host,port,version,capabilities_json,status,registered_at,last_heartbeat_at FROM agents ORDER BY registered_at DESC");
        let rows = sqlx::query(
            "SELECT agent_id, agent_name, host, port, version, capabilities_json, status, registered_at, last_heartbeat_at FROM agents ORDER BY registered_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        let agents = rows
            .into_iter()
            .map(|row| {
                let capabilities_json: String = row.get(5);
                let capabilities: AgentCapabilities =
                    serde_json::from_str(&capabilities_json).unwrap_or(AgentCapabilities {
                        can_collect: false,
                        can_parse: false,
                        can_load: false,
                        supported_protocols: vec![],
                    });
                let status_str: String = row.get(6);
                let status = match status_str.as_str() {
                    "ONLINE" => AgentStatus::Online,
                    "OFFLINE" => AgentStatus::Offline,
                    _ => AgentStatus::Unknown,
                };
                AgentInfo {
                    agent_id: row.get(0),
                    agent_name: row.get(1),
                    host: row.get(2),
                    port: row.get(3),
                    version: row.get(4),
                    capabilities,
                    status,
                    registered_at: row.get(7),
                    last_heartbeat_at: row.get(8),
                }
            })
            .collect();
        Ok(agents)
    }

    pub async fn list_online_agents(&self) -> Result<Vec<OnlineAgent>> {
        let rows = sqlx::query(
            "SELECT agent_id, host, port FROM agents WHERE status = 'ONLINE'",
        )
        .fetch_all(&self.pool)
        .await?;
        let agents = rows
            .into_iter()
            .map(|row| OnlineAgent {
                agent_id: row.get(0),
                host: row.get(1),
                port: row.get(2),
            })
            .collect();
        Ok(agents)
    }

    pub async fn insert_config_snapshot(&self, snapshot: &ConfigSnapshotResponse) -> Result<()> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        sqlx::query(
            "INSERT OR REPLACE INTO config_snapshots(config_snapshot_id, content_hash, snapshot_json, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&snapshot.config_snapshot_id)
        .bind(&snapshot.content_hash)
        .bind(serde_json::to_string(snapshot)?)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_config_snapshot_meta(
        &self,
        snapshot_id: &str,
        content_hash: &str,
        version_label: &str,
        file_count: usize,
    ) -> Result<()> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        sqlx::query(
            "INSERT OR REPLACE INTO config_snapshots(config_snapshot_id, content_hash, version_label, is_active, file_count, snapshot_json, created_at, activated_at) VALUES (?, ?, ?, 0, ?, '{}', ?, NULL)",
        )
        .bind(snapshot_id)
        .bind(content_hash)
        .bind(version_label)
        .bind(file_count as i64)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_config_snapshots(&self) -> Result<Vec<ConfigSnapshotMeta>> {
        let rows = sqlx::query(
            "SELECT config_snapshot_id, content_hash, version_label, is_active, file_count, created_at, activated_at FROM config_snapshots ORDER BY created_at DESC, config_snapshot_id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        let snapshots = rows
            .into_iter()
            .map(|row| ConfigSnapshotMeta {
                config_snapshot_id: row.get(0),
                content_hash: row.get(1),
                version_label: row.get(2),
                is_active: row.get::<i32, _>(3) != 0,
                file_count: row.get::<i64, _>(4) as usize,
                created_at: row.get(5),
                activated_at: row.get(6),
            })
            .collect();
        Ok(snapshots)
    }

    pub async fn get_config_snapshot(&self, snapshot_id: &str) -> Result<Option<ConfigSnapshotMeta>> {
        let row = sqlx::query(
            "SELECT config_snapshot_id, content_hash, version_label, is_active, file_count, created_at, activated_at FROM config_snapshots WHERE config_snapshot_id = ?",
        )
        .bind(snapshot_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|rw| ConfigSnapshotMeta {
            config_snapshot_id: rw.get(0),
            content_hash: rw.get(1),
            version_label: rw.get(2),
            is_active: rw.get::<i32, _>(3) != 0,
            file_count: rw.get::<i64, _>(4) as usize,
            created_at: rw.get(5),
            activated_at: rw.get(6),
        }))
    }

    pub async fn activate_config_snapshot(&self, snapshot_id: &str) -> Result<ConfigSnapshotMeta> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        sqlx::query("UPDATE config_snapshots SET is_active = 0")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "UPDATE config_snapshots SET is_active = 1, activated_at = ? WHERE config_snapshot_id = ?",
        )
        .bind(&now)
        .bind(snapshot_id)
        .execute(&self.pool)
        .await?;
        self.get_config_snapshot(snapshot_id).await?.ok_or_else(|| anyhow::anyhow!("snapshot {snapshot_id} not found"))
    }

    pub async fn create_task(
        &self,
        task_id: &str,
        logical_task_key: &str,
        strategy_id: &str,
        config_snapshot_id: &str,
        scan_start_time: &str,
        collect_id: &str,
        assigned_agent_id: &str,
    ) -> Result<()> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        sqlx::query(
            "INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'CREATED', ?)",
        )
        .bind(task_id)
        .bind(logical_task_key)
        .bind(strategy_id)
        .bind(config_snapshot_id)
        .bind(scan_start_time)
        .bind(collect_id)
        .bind(assigned_agent_id)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn accept_task_result(&self, report: &TaskResultReport) -> Result<()> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let task_row = sqlx::query(
            "SELECT strategy_id, config_snapshot_id, status FROM collect_tasks WHERE task_id = ?",
        )
        .bind(&report.task_id)
        .fetch_optional(&self.pool)
        .await?;

        let (strategy_id, config_snapshot_id) = match task_row {
            Some(row) => {
                let sid: String = row.get(0);
                let cid: String = row.get(1);
                let status: String = row.get(2);
                tracing::info!(
                    "[core-db] accept_task_result: existing task status={status} strategy={sid}"
                );
                match status.as_str() {
                    "SUCCEEDED" | "FAILED" | "TIMEOUT" | "CANCELLED" => {
                        anyhow::bail!(
                            "task {} is already in terminal state {}",
                            report.task_id,
                            status
                        );
                    }
                    _ => {}
                }
                (sid, cid)
            }
            None => {
                tracing::info!(
                    "[core-db] accept_task_result: task not found, creating implicit record"
                );
                let sid = format!("unknown_{}", report.task_id);
                let cid = "unknown".to_string();
                sqlx::query(
                    "INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'CREATED', ?)",
                )
                .bind(&report.task_id)
                .bind("")
                .bind(&sid)
                .bind(&cid)
                .bind("")
                .bind("")
                .bind(&report.agent_id)
                .bind(&now)
                .execute(&self.pool)
                .await?;
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

        tracing::info!(
            "[core-db] inserting {} result cells for task {} (strategy={})",
            report.result_rows.len(),
            report.task_id,
            strategy_id
        );
        for result in &report.result_rows {
            tracing::debug!(
                "[core-db]   cell: table={} data_time={} rows={} success={}",
                result.table_name,
                result.data_time,
                result.row_count,
                result.success
            );
            sqlx::query(
                "INSERT INTO collect_result_cells(task_id, strategy_id, agent_id, config_snapshot_id, table_name, data_time, row_count, success, collect_time, status, error_message, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'SUCCEEDED', NULL, ?, ?)",
            )
            .bind(&report.task_id)
            .bind(&strategy_id)
            .bind(&report.agent_id)
            .bind(&config_snapshot_id)
            .bind(&result.table_name)
            .bind(&result.data_time)
            .bind(result.row_count as i64)
            .bind(result.success)
            .bind(&result.collect_time)
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            "UPDATE collect_tasks SET status = ?, finished_at = ? WHERE task_id = ?",
        )
        .bind(terminal_status)
        .bind(&now)
        .bind(&report.task_id)
        .execute(&self.pool)
        .await?;
        tracing::info!("[core-db] accept_task_result done: status={terminal_status}");
        Ok(())
    }

    pub async fn result_rows_for_day(
        &self,
        strategy_id: &str,
        day: &str,
    ) -> Result<Vec<ResultRow>> {
        let like = format!("{day}%");
        let rows = sqlx::query(
            "SELECT table_name, data_time, row_count, success, collect_time FROM collect_result_cells WHERE strategy_id = ? AND data_time LIKE ? ORDER BY table_name, data_time",
        )
        .bind(strategy_id)
        .bind(&like)
        .fetch_all(&self.pool)
        .await?;
        let results = rows
            .into_iter()
            .map(|row| ResultRow {
                table_name: row.get(0),
                data_time: row.get(1),
                row_count: row.get::<i64, _>(2) as u64,
                success: row.get(3),
                collect_time: row.get(4),
            })
            .collect();
        Ok(results)
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

    async fn db() -> TestDb {
        let dir = tempdir().unwrap();
        let db = CoreDb::open(dir.path().join("core.db")).await.unwrap();
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

    #[tokio::test]
    async fn registers_agent_and_reuses_existing_agent_id() {
        let db = db().await;
        let agent_id = db.register_agent(&agent_request()).await.unwrap();
        let mut reconnect = agent_request();
        reconnect.agent_id = Some(agent_id.clone());
        let reused = db.register_agent(&reconnect).await.unwrap();
        assert_eq!(reused, agent_id);
    }

    #[tokio::test]
    async fn stores_task_result_rows() {
        let db = db().await;
        let agent_id = db.register_agent(&agent_request()).await.unwrap();
        db.insert_config_snapshot(&ConfigSnapshotResponse {
            config_snapshot_id: "cfg_1".to_string(),
            content_hash: "sha256:test".to_string(),
            source_toml: "[source]".to_string(),
            mapping_dx_ini: "[m]".to_string(),
            load_toml: "[load]".to_string(),
            col_name_cut_config_ini: None,
            rules: vec![RuleFile {
                relative_path: "rules/a.json".to_string(),
                content: "{\"table_name\":\"TPD_A\"}".to_string(),
            }],
        })
        .await
        .unwrap();
        db.create_task(
            "task_1",
            "strategy_1:2026-06-17 15:15:00:cfg_1",
            "strategy_1",
            "cfg_1",
            "2026-06-17 15:15:00",
            "collect_1",
            &agent_id,
        )
        .await
        .unwrap();
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
        })
        .await
        .unwrap();
        let rows = db.result_rows_for_day("strategy_1", "2026-06-17").await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].table_name, "TPD_A");
        assert_eq!(rows[0].row_count, 123);
        let stored_status: (String,) = sqlx::query_as("SELECT status FROM collect_tasks WHERE task_id = ?")
            .bind("task_1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(stored_status.0, "SUCCEEDED");
    }

    #[tokio::test]
    async fn inserts_and_lists_config_snapshots() {
        let db = db().await;
        db.insert_config_snapshot_meta("v1", "sha256:aaa", "v1_label", 5)
            .await
            .unwrap();
        db.insert_config_snapshot_meta("v2", "sha256:bbb", "v2_label", 3)
            .await
            .unwrap();
        let list = db.list_config_snapshots().await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].config_snapshot_id, "v2");
    }

    #[tokio::test]
    async fn lists_online_agents() {
        let db = db().await;
        let mut req = agent_request();
        req.agent_id = Some("agent_a".into());
        db.register_agent(&req).await.unwrap();
        let agents = db.list_online_agents().await.unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "agent_a");
        assert_eq!(agents[0].host, "127.0.0.1");
        assert_eq!(agents[0].port, 18081);
    }

    #[tokio::test]
    async fn activate_switches_snapshot_is_active() {
        let db = db().await;
        db.insert_config_snapshot_meta("v1", "sha256:aaa", "v1_label", 5)
            .await
            .unwrap();
        db.insert_config_snapshot_meta("v2", "sha256:bbb", "v2_label", 3)
            .await
            .unwrap();
        let meta = db.activate_config_snapshot("v1").await.unwrap();
        assert!(meta.is_active);
        let v1 = db.get_config_snapshot("v1").await.unwrap().unwrap();
        assert!(v1.is_active);
        let v2 = db.get_config_snapshot("v2").await.unwrap().unwrap();
        assert!(!v2.is_active);
    }
}
