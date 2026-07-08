use std::path::Path;

use anyhow::Result;
use sqlx::Row;
use sqlx::SqlitePool;

use crate::core_agent_api::{
    AgentDispatchCandidate, AgentGroupRow, AgentInfoRow,
    AgentStatusHisRow, AgentStatusRow, CollectionStrategyCreateRequest, CollectionStrategyRow,
    CollectionStrategyUpdateRequest, ConfigNameItem, ConfigSnapshotMeta, ConfigSnapshotResponse,
    DataCollectorUnitRow, DataCollectorUnitSaveRequest, ResultRow, TaskResultReport,
    TaskStatus,
};

/// Log a SQL query with optional named parameters and log level.
///
/// ```ignore
/// trace_sql!("SELECT * FROM t WHERE id = ?", id = val);
/// trace_sql!(warn, "DELETE FROM t WHERE id = ?", id = val);
/// trace_sql!("SELECT * FROM t"); // no params
/// trace_sql!(debug, "SELECT 1"); // debug level, no params
/// ```
macro_rules! trace_sql {
    // --- info (default) ---
    ($sql:expr $(,)?) => {
        tracing::info!("[db] ==> {}", $sql);
    };
    ($sql:expr $(, $key:ident = $val:expr)+ $(,)?) => {{
        tracing::info!("[db] ==> {}", $sql);
        let _p: Vec<String> = vec![$(format!("{}={:?}", stringify!($key), $val)),*];
        tracing::info!("[db] ==> Parameters: {}", _p.join(", "));
    }};
    // --- dispatch by level ---
    ($level:ident, $($rest:tt)*) => {
        trace_sql!(@inner $level, $($rest)*)
    };
    // --- info ---
    (@inner info, $sql:expr $(,)?) => {
        tracing::info!("[db] ==> {}", $sql);
    };
    (@inner info, $sql:expr $(, $key:ident = $val:expr)+ $(,)?) => {{
        tracing::info!("[db] ==> {}", $sql);
        let _p: Vec<String> = vec![$(format!("{}={:?}", stringify!($key), $val)),*];
        tracing::info!("[db] ==> Parameters: {}", _p.join(", "));
    }};
    // --- debug ---
    (@inner debug, $sql:expr $(,)?) => {
        tracing::debug!("[db] ==> {}", $sql);
    };
    (@inner debug, $sql:expr $(, $key:ident = $val:expr)+ $(,)?) => {{
        tracing::debug!("[db] ==> {}", $sql);
        let _p: Vec<String> = vec![$(format!("{}={:?}", stringify!($key), $val)),*];
        tracing::debug!("[db] ==> Parameters: {}", _p.join(", "));
    }};
    // --- warn ---
    (@inner warn, $sql:expr $(,)?) => {
        tracing::warn!("[db] ==> {}", $sql);
    };
    (@inner warn, $sql:expr $(, $key:ident = $val:expr)+ $(,)?) => {{
        tracing::warn!("[db] ==> {}", $sql);
        let _p: Vec<String> = vec![$(format!("{}={:?}", stringify!($key), $val)),*];
        tracing::warn!("[db] ==> Parameters: {}", _p.join(", "));
    }};
    // --- error ---
    (@inner error, $sql:expr $(,)?) => {
        tracing::error!("[db] ==> {}", $sql);
    };
    (@inner error, $sql:expr $(, $key:ident = $val:expr)+ $(,)?) => {{
        tracing::error!("[db] ==> {}", $sql);
        let _p: Vec<String> = vec![$(format!("{}={:?}", stringify!($key), $val)),*];
        tracing::error!("[db] ==> Parameters: {}", _p.join(", "));
    }};
}

#[allow(dead_code)]
const NON_TERMINAL_TASK_STATUS_SQL: &str = "status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')";

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
        sqlx::query("DROP TABLE IF EXISTS agents")
            .execute(&self.pool).await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agent_info (
                agent_id            INTEGER PRIMARY KEY,
                agent_name          TEXT NOT NULL,
                agent_ip            TEXT NOT NULL,
                port                INTEGER NOT NULL,
                version             TEXT NOT NULL,
                cpu_total           TEXT,
                memory_total        REAL,
                disk_total          REAL,
                heartbeat_interval  INTEGER,
                time_stamp          TEXT DEFAULT (datetime('now','localtime')),
                description         TEXT,
                max_thread_num      INTEGER,
                agent_isuse_flag    INTEGER NOT NULL DEFAULT 1,
                fact_memory_total   REAL,
                agent_alias         TEXT,
                is_core             INTEGER NOT NULL DEFAULT 0,
                agent_power         REAL DEFAULT 1.0,
                host_load_limit     REAL DEFAULT 90.0,
                registered_at       TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool).await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agent_status (
                agent_id          INTEGER PRIMARY KEY,
                status            TEXT NOT NULL,
                cpu_load          REAL,
                memory_load       REAL,
                disk_load         REAL,
                heartbeat_time    TEXT NOT NULL,
                thread_num        INTEGER,
                description       TEXT
            )
            "#,
        )
        .execute(&self.pool).await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agent_status_his (
                agent_id          INTEGER NOT NULL,
                cpu_load          REAL,
                memory_load       REAL,
                disk_load         REAL,
                heartbeat_time    TEXT NOT NULL,
                thread_num        INTEGER,
                description       TEXT,
                insert_time       TEXT DEFAULT (datetime('now','localtime'))
            )
            "#,
        )
        .execute(&self.pool).await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_agent_status_his_agent_time
                ON agent_status_his(agent_id, heartbeat_time)
            "#,
        )
        .execute(&self.pool).await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agent_group (
                group_id    INTEGER PRIMARY KEY AUTOINCREMENT,
                group_name  TEXT NOT NULL,
                agent_ids   TEXT DEFAULT '[]' NOT NULL,
                description TEXT,
                time_stamp  TEXT
            )
            "#,
        )
        .execute(&self.pool).await?;
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
                error_message TEXT,
                group_id TEXT,
                retry_count INTEGER NOT NULL DEFAULT 0,
                next_retry_at TEXT,
                dispatch_error TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        trace_sql!("ALTER TABLE collect_tasks ADD COLUMN group_id TEXT");
        let _ = sqlx::query("ALTER TABLE collect_tasks ADD COLUMN group_id TEXT")
            .execute(&self.pool)
            .await;
        trace_sql!("ALTER TABLE collect_tasks ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0");
        let _ = sqlx::query("ALTER TABLE collect_tasks ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0")
            .execute(&self.pool)
            .await;
        trace_sql!("ALTER TABLE collect_tasks ADD COLUMN next_retry_at TEXT");
        let _ = sqlx::query("ALTER TABLE collect_tasks ADD COLUMN next_retry_at TEXT")
            .execute(&self.pool)
            .await;
        trace_sql!("ALTER TABLE collect_tasks ADD COLUMN dispatch_error TEXT");
        let _ = sqlx::query("ALTER TABLE collect_tasks ADD COLUMN dispatch_error TEXT")
            .execute(&self.pool)
            .await;
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
        let _ = sqlx::query("ALTER TABLE config_snapshots ADD COLUMN name TEXT")
            .execute(&self.pool)
        .await;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS config_tables (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                config_snapshot_id TEXT NOT NULL,
                config_name TEXT NOT NULL,
                table_name TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS data_collector_unit (
                id INTEGER PRIMARY KEY,
                unit_name TEXT NOT NULL,
                config_name TEXT NOT NULL,
                config_version TEXT NOT NULL DEFAULT '',
                table_names TEXT NOT NULL DEFAULT '[]',
                agent_ids TEXT NOT NULL DEFAULT '[]',
                data_interval_seconds INTEGER NOT NULL DEFAULT 900,
                collector_interval INTEGER NOT NULL DEFAULT 900,
                task_timeout_seconds INTEGER NOT NULL DEFAULT 3600,
                source_type TEXT NOT NULL DEFAULT 'sftp',
                file_encoding TEXT NOT NULL DEFAULT 'UTF-8',
                remote_pattern TEXT NOT NULL DEFAULT '',
                host TEXT NOT NULL DEFAULT '',
                port INTEGER NOT NULL DEFAULT 22,
                username TEXT NOT NULL DEFAULT '',
                password TEXT NOT NULL DEFAULT '',
                connect_retry INTEGER NOT NULL DEFAULT 3,
                download_retry INTEGER NOT NULL DEFAULT 3,
                download_parallel INTEGER NOT NULL DEFAULT 4,
                retry_interval_secs INTEGER NOT NULL DEFAULT 30,
                connect_timeout_secs INTEGER NOT NULL DEFAULT 30,
                read_timeout_secs INTEGER NOT NULL DEFAULT 300,
                cache_retention_days INTEGER NOT NULL DEFAULT 7,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        // ── Auto-dispatch columns ──
        sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN load_type TEXT NOT NULL DEFAULT 'clickhouse'")
            .execute(&self.pool).await.ok();
        sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN output_delimiter TEXT NOT NULL DEFAULT '|'")
            .execute(&self.pool).await.ok();
        sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_host TEXT NOT NULL DEFAULT ''")
            .execute(&self.pool).await.ok();
        sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_port INTEGER NOT NULL DEFAULT 9000")
            .execute(&self.pool).await.ok();
        sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_user TEXT NOT NULL DEFAULT ''")
            .execute(&self.pool).await.ok();
        sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_password TEXT NOT NULL DEFAULT ''")
            .execute(&self.pool).await.ok();
        sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_database TEXT NOT NULL DEFAULT ''")
            .execute(&self.pool).await.ok();
        sqlx::query("ALTER TABLE data_collector_unit ADD COLUMN db_table_name_case TEXT NOT NULL DEFAULT 'lower'")
            .execute(&self.pool).await.ok();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS collection_strategy (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                collector_name TEXT NOT NULL,
                collector_id INTEGER NOT NULL,
                table_name TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT '可用',
                cron_expression TEXT NOT NULL DEFAULT '',
                collect_interval INTEGER NOT NULL,
                data_interval INTEGER NOT NULL,
                data_start_time TEXT,
                data_end_time TEXT,
                execute_time TEXT,
                agent_ids TEXT NOT NULL DEFAULT '[]',
                strategy_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn select_online_agent(&self) -> Result<(i64, f64)> {
        trace_sql!("SELECT ai.agent_id, COALESCE(ai.agent_power, 1.0) FROM agent_info ai JOIN agent_status ast ON ast.agent_id = ai.agent_id WHERE ast.status = 'ONLINE' AND ai.agent_isuse_flag = 1 ORDER BY ast.heartbeat_time DESC LIMIT 1");
        let row = sqlx::query_as::<_, (i64, f64)>(
            r#"
            SELECT ai.agent_id, COALESCE(ai.agent_power, 1.0)
            FROM agent_info ai
            JOIN agent_status ast ON ast.agent_id = ai.agent_id
            WHERE ast.status = 'ONLINE'
              AND ai.agent_isuse_flag = 1
            ORDER BY ast.heartbeat_time DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        row.ok_or_else(|| anyhow::anyhow!("no online agent available"))
    }

    pub async fn insert_config_snapshot(&self, snapshot: &ConfigSnapshotResponse) -> Result<()> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        trace_sql!("INSERT OR REPLACE INTO config_snapshots(config_snapshot_id, content_hash, snapshot_json, created_at) VALUES (?, ?, ?, ?)", config_snapshot_id = snapshot.config_snapshot_id, content_hash = snapshot.content_hash);
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
        name: &str,
        table_names: &[String],
    ) -> Result<()> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        trace_sql!("INSERT OR REPLACE INTO config_snapshots(config_snapshot_id, content_hash, version_label, is_active, file_count, name, snapshot_json, created_at, activated_at) VALUES (?, ?, ?, 0, ?, ?, '{{}}', ?, NULL)", snapshot_id = snapshot_id, content_hash = content_hash, version_label = version_label, file_count = file_count, name = name);
        sqlx::query(
            "INSERT OR REPLACE INTO config_snapshots(config_snapshot_id, content_hash, version_label, is_active, file_count, name, snapshot_json, created_at, activated_at) VALUES (?, ?, ?, 0, ?, ?, '{}', ?, NULL)",
        )
        .bind(snapshot_id)
        .bind(content_hash)
        .bind(version_label)
        .bind(file_count as i64)
        .bind(name)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        for table_name in table_names {
            trace_sql!("INSERT INTO config_tables(config_snapshot_id, config_name, table_name) VALUES (?, ?, ?)", snapshot_id = snapshot_id, config_name = name, table_name = table_name);
            sqlx::query(
                "INSERT INTO config_tables(config_snapshot_id, config_name, table_name) VALUES (?, ?, ?)",
            )
            .bind(snapshot_id)
            .bind(name)
            .bind(table_name)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn list_config_snapshots(&self) -> Result<Vec<ConfigSnapshotMeta>> {
        trace_sql!("SELECT config_snapshot_id, content_hash, version_label, is_active, file_count, name, created_at, activated_at FROM config_snapshots ORDER BY created_at DESC, config_snapshot_id DESC");
        let rows = sqlx::query(
            "SELECT config_snapshot_id, content_hash, version_label, is_active, file_count, name, created_at, activated_at FROM config_snapshots ORDER BY created_at DESC, config_snapshot_id DESC",
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
                name: row.get(5),
                created_at: row.get(6),
                activated_at: row.get(7),
            })
            .collect();
        Ok(snapshots)
    }

    pub async fn get_config_snapshot(&self, snapshot_id: &str) -> Result<Option<ConfigSnapshotMeta>> {
        trace_sql!("SELECT config_snapshot_id, content_hash, version_label, is_active, file_count, name, created_at, activated_at FROM config_snapshots WHERE config_snapshot_id = ?", snapshot_id = snapshot_id);
        let row = sqlx::query(
            "SELECT config_snapshot_id, content_hash, version_label, is_active, file_count, name, created_at, activated_at FROM config_snapshots WHERE config_snapshot_id = ?",
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
            name: rw.get(5),
            created_at: rw.get(6),
            activated_at: rw.get(7),
        }))
    }

    pub async fn activate_config_snapshot(&self, snapshot_id: &str) -> Result<ConfigSnapshotMeta> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        trace_sql!("UPDATE config_snapshots SET is_active = 0");
        sqlx::query("UPDATE config_snapshots SET is_active = 0")
            .execute(&self.pool)
            .await?;
        trace_sql!("UPDATE config_snapshots SET is_active = 1, activated_at = ? WHERE config_snapshot_id = ?", snapshot_id = snapshot_id);
        sqlx::query(
            "UPDATE config_snapshots SET is_active = 1, activated_at = ? WHERE config_snapshot_id = ?",
        )
        .bind(&now)
        .bind(snapshot_id)
        .execute(&self.pool)
        .await?;
        let meta = self.get_config_snapshot(snapshot_id).await?.ok_or_else(|| anyhow::anyhow!("snapshot {snapshot_id} not found"))?;
        if let Some(ref name) = meta.name {
            trace_sql!("UPDATE data_collector_unit SET config_version = ? WHERE config_name = ? AND config_version != ?", config_version = meta.config_snapshot_id, config_name = name);
            sqlx::query("UPDATE data_collector_unit SET config_version = ? WHERE config_name = ? AND config_version != ?")
                .bind(&meta.config_snapshot_id)
                .bind(name)
                .bind(&meta.config_snapshot_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(meta)
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
        group_id: &str,
    ) -> Result<()> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        trace_sql!("INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, group_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'CREATED', ?)", task_id = task_id, logical_task_key = logical_task_key, strategy_id = strategy_id, config_snapshot_id = config_snapshot_id, scan_start_time = scan_start_time, collect_id = collect_id, assigned_agent_id = assigned_agent_id, group_id = group_id);
        sqlx::query(
            "INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, group_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'CREATED', ?)",
        )
        .bind(task_id)
        .bind(logical_task_key)
        .bind(strategy_id)
        .bind(config_snapshot_id)
        .bind(scan_start_time)
        .bind(collect_id)
        .bind(assigned_agent_id)
        .bind(group_id)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn assign_group_to_agent(&self, group_id: &str, agent_id: &str) -> Result<u64> {
        trace_sql!("UPDATE collect_tasks SET assigned_agent_id = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')", agent_id = agent_id, group_id = group_id);
        let result = sqlx::query(
            "UPDATE collect_tasks SET assigned_agent_id = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')",
        )
        .bind(agent_id)
        .bind(group_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn update_group_status(&self, group_id: &str, status: &str, error_message: Option<&str>) -> Result<u64> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("UPDATE collect_tasks SET status = ?, dispatch_error = ?, last_progress_at = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')", status = status, error_message = error_message, group_id = group_id);
        let result = sqlx::query(
            "UPDATE collect_tasks SET status = ?, dispatch_error = ?, last_progress_at = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')",
        )
        .bind(status)
        .bind(error_message)
        .bind(&now)
        .bind(group_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn update_task_status(&self, task_id: &str, status: &str, error_message: Option<&str>) -> Result<u64> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("UPDATE collect_tasks SET status = ?, last_progress_at = ?, dispatch_error = ? WHERE task_id = ?", status = status, task_id = task_id, error_message = error_message);
        let result = sqlx::query(
            "UPDATE collect_tasks SET status = ?, last_progress_at = ?, dispatch_error = ? WHERE task_id = ?",
        )
        .bind(status)
        .bind(&now)
        .bind(error_message)
        .bind(task_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn increment_group_retry(&self, group_id: &str, next_retry_at: &str, error_message: &str) -> Result<u64> {
        trace_sql!("UPDATE collect_tasks SET retry_count = retry_count + 1, next_retry_at = ?, dispatch_error = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')", next_retry_at = next_retry_at, error_message = error_message, group_id = group_id);
        let result = sqlx::query(
            "UPDATE collect_tasks SET retry_count = retry_count + 1, next_retry_at = ?, dispatch_error = ? WHERE group_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')",
        )
        .bind(next_retry_at)
        .bind(error_message)
        .bind(group_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn count_active_tasks_by_agent(&self, agent_id: &str) -> Result<i64> {
        trace_sql!("SELECT COUNT(*) FROM collect_tasks WHERE assigned_agent_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')", agent_id = agent_id);
        let count = sqlx::query_scalar(
            "SELECT COUNT(*) FROM collect_tasks WHERE assigned_agent_id = ? AND status NOT IN ('SUCCEEDED', 'FAILED', 'TIMEOUT', 'CANCELLED')",
        )
        .bind(agent_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    pub async fn mark_active_tasks_failed_for_agent(&self, agent_id: &str, reason: &str) -> Result<u64> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("UPDATE collect_tasks SET status = 'FAILED', finished_at = ?, dispatch_error = ? WHERE assigned_agent_id = ? AND status IN ('CREATED', 'DISPATCHING', 'ACCEPTED', 'RUNNING')", agent_id = agent_id, reason = reason);
        let result = sqlx::query(
            "UPDATE collect_tasks SET status = 'FAILED', finished_at = ?, dispatch_error = ? WHERE assigned_agent_id = ? AND status IN ('CREATED', 'DISPATCHING', 'ACCEPTED', 'RUNNING')",
        )
        .bind(&now)
        .bind(reason)
        .bind(agent_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn accept_task_result(&self, report: &TaskResultReport) -> Result<()> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        trace_sql!("SELECT strategy_id, config_snapshot_id, status, scan_start_time FROM collect_tasks WHERE task_id = ?", task_id = report.task_id);
        let task_row = sqlx::query(
            "SELECT strategy_id, config_snapshot_id, status, scan_start_time FROM collect_tasks WHERE task_id = ?",
        )
        .bind(&report.task_id)
        .fetch_optional(&self.pool)
        .await?;

        let (strategy_id, config_snapshot_id, scan_start_time) = match task_row {
            Some(row) => {
                let sid: String = row.get(0);
                let cid: String = row.get(1);
                let status: String = row.get(2);
                let sst: String = row.get(3);
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
                (sid, cid, Some(sst))
            }
            None => {
                tracing::info!(
                    "[core-db] accept_task_result: task not found, creating implicit record"
                );
                let sid = format!("unknown_{}", report.task_id);
                let cid = "unknown".to_string();
                trace_sql!("INSERT INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collect_id, assigned_agent_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'CREATED', ?)", task_id = report.task_id, agent_id = report.agent_id);
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
        trace_sql!("ALTER TABLE config_tables ADD COLUMN config_snapshot_id TEXT");
        let _ = sqlx::query("ALTER TABLE config_tables ADD COLUMN config_snapshot_id TEXT")
            .execute(&self.pool)
            .await;
                (sid, cid, None)
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
            tracing::info!(
                "[core-db]   cell: table={} data_time={} rows={} success={}",
                result.table_name,
                result.data_time,
                result.row_count,
                result.success
            );
            trace_sql!("INSERT INTO collect_result_cells(task_id, strategy_id, agent_id, config_snapshot_id, table_name, data_time, row_count, success, collect_time, ...)");
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

        // 如果任务是失败/超时/取消状态但没有结果行，创建一条合成失败记录
        if report.result_rows.is_empty() {
            let sid_int: i64 = strategy_id.parse().unwrap_or(0);
            trace_sql!("SELECT table_name FROM collection_strategy WHERE id = ?", id = sid_int);
            let table_name: Option<String> = sqlx::query_scalar(
                "SELECT table_name FROM collection_strategy WHERE id = ?",
            )
            .bind(sid_int)
            .fetch_optional(&self.pool)
            .await?;
            if let Some(tn) = table_name {
                let data_time = scan_start_time.unwrap_or_else(|| now.clone());
                tracing::info!(
                    "[core-db] inserting synthetic failure cell for table={} strategy={}",
                    tn,
                    strategy_id
                );
                let is_failure = terminal_status == "FAILED" || terminal_status == "TIMEOUT" || terminal_status == "CANCELLED";
                let success = if is_failure { 0i64 } else { 1i64 };
                let cell_status = if is_failure { "FAILED" } else { "SUCCEEDED" };
                trace_sql!("INSERT INTO collect_result_cells(...) synthetic failure cell");
                sqlx::query(
                    "INSERT INTO collect_result_cells(task_id, strategy_id, agent_id, config_snapshot_id, table_name, data_time, row_count, success, collect_time, status, error_message, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?, NULL, ?, ?)",
                )
                .bind(&report.task_id)
                .bind(&strategy_id)
                .bind(&report.agent_id)
                .bind(&config_snapshot_id)
                .bind(&tn)
                .bind(&data_time)
                .bind(success)
                .bind(&now)
                .bind(cell_status)
                .bind(&now)
                .bind(&now)
                .execute(&self.pool)
                .await?;
            }
        }

        trace_sql!("UPDATE collect_tasks SET status = ?, finished_at = ? WHERE task_id = ?", status = terminal_status, task_id = report.task_id);
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
        trace_sql!("SELECT table_name, data_time, row_count, success, collect_time FROM collect_result_cells WHERE strategy_id = ? AND data_time LIKE ? ORDER BY table_name, data_time", strategy_id = strategy_id, day = day);
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

    pub async fn next_unit_id(&self) -> Result<i64> {
        Ok(1)
    }

    pub async fn next_strategy_id(&self) -> Result<i64> {
        trace_sql!("SELECT COALESCE(MAX(id), 0) + 1 FROM collection_strategy");
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM collection_strategy",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn create_strategies(
        &self,
        req: &CollectionStrategyCreateRequest,
    ) -> Result<Vec<CollectionStrategyRow>> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        tracing::info!("[db] ==> INSERT INTO collection_strategy ... ({} tables)", req.table_names.len());
        for table_name in &req.table_names {
            trace_sql!("INSERT INTO collection_strategy (collector_name, collector_id, table_name, ...) VALUES (?, ?, ?, ...)", table_name = table_name);
            sqlx::query(
                "INSERT INTO collection_strategy (collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at) VALUES (?, ?, ?, '可用', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&req.collector_name)
            .bind(req.collector_id)
            .bind(table_name)
            .bind(req.cron_expression.as_deref().unwrap_or(""))
            .bind(req.collect_interval)
            .bind(req.data_interval)
            .bind(&req.data_start_time)
            .bind(&req.data_end_time)
            .bind(&req.execute_time)
            .bind(&req.agent_ids)
            .bind(&req.strategy_type)
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        }
        trace_sql!("SELECT id FROM collection_strategy ORDER BY id DESC LIMIT ?", limit = req.table_names.len());
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM collection_strategy ORDER BY id DESC LIMIT ?",
        )
        .bind(req.table_names.len() as i64)
        .fetch_all(&self.pool)
        .await?;
        let mut rows = Vec::new();
        for id in ids.iter().rev() {
            rows.push(self.get_strategy(*id).await?.unwrap());
        }
        Ok(rows)
    }

    pub async fn get_strategy(&self, id: i64) -> Result<Option<CollectionStrategyRow>> {
        trace_sql!("SELECT * FROM collection_strategy WHERE id = ?", id = id);
        let row = sqlx::query_as::<_, CollectionStrategyRow>(
            "SELECT id, collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at FROM collection_strategy WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_strategies(
        &self,
        collector_name: Option<&str>,
        strategy_type: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<CollectionStrategyRow>> {
        let collector_name = collector_name.map(|s| format!("%{}%", s));
        let strategy_type = strategy_type.map(|s| s.to_string());
        let status = status.map(|s| s.to_string());

        let mut sql = String::from(
            "SELECT id, collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at FROM collection_strategy WHERE 1=1",
        );
        tracing::info!("[db] ==> {} (collector_name={:?}, strategy_type={:?}, status={:?})", sql, collector_name, strategy_type, status);
        if collector_name.is_some() {
            sql.push_str(" AND collector_name LIKE ?");
        }
        if strategy_type.is_some() {
            sql.push_str(" AND strategy_type = ?");
        }
        if status.is_some() {
            sql.push_str(" AND status = ?");
        }
        sql.push_str(" ORDER BY id DESC");

        let mut query = sqlx::query_as::<_, CollectionStrategyRow>(&sql);
        if let Some(ref v) = collector_name {
            query = query.bind(v);
        }
        if let Some(ref v) = strategy_type {
            query = query.bind(v);
        }
        if let Some(ref v) = status {
            query = query.bind(v);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows)
    }

    pub async fn update_strategy(
        &self,
        id: i64,
        req: &CollectionStrategyUpdateRequest,
    ) -> Result<bool> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        tracing::info!("[db] ==> UPDATE collection_strategy SET ... WHERE id = {}", id);
        let mut sql = String::from("UPDATE collection_strategy SET updated_at = ?");
        let mut values: Vec<String> = vec![now];

        if let Some(ref v) = req.cron_expression {
            sql.push_str(", cron_expression = ?");
            values.push(v.clone());
        }
        if let Some(v) = req.collect_interval {
            sql.push_str(", collect_interval = ?");
            values.push(v.to_string());
        }
        if let Some(v) = req.data_interval {
            sql.push_str(", data_interval = ?");
            values.push(v.to_string());
        }
        if let Some(ref v) = req.data_start_time {
            sql.push_str(", data_start_time = ?");
            values.push(v.clone());
        }
        if let Some(ref v) = req.data_end_time {
            sql.push_str(", data_end_time = ?");
            values.push(v.clone());
        }
        if let Some(ref v) = req.execute_time {
            sql.push_str(", execute_time = ?");
            values.push(v.clone());
        }
        if let Some(ref v) = req.agent_ids {
            sql.push_str(", agent_ids = ?");
            values.push(v.clone());
        }
        if let Some(ref v) = req.status {
            sql.push_str(", status = ?");
            values.push(v.clone());
        }
        sql.push_str(" WHERE id = ?");
        values.push(id.to_string());

        let mut query = sqlx::query(&sql);
        for v in values {
            query = query.bind(v);
        }
        let result = query.execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn batch_suspend(&self, ids: &[i64]) -> Result<usize> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        tracing::info!("[db] ==> batch suspend {} strategies", ids.len());
        let mut count = 0;
        for id in ids {
            let r = sqlx::query(
                "UPDATE collection_strategy SET status = '挂起', updated_at = ? WHERE id = ?",
            )
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
            count += r.rows_affected() as usize;
        }
        Ok(count)
    }

    pub async fn batch_activate(&self, ids: &[i64]) -> Result<usize> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        tracing::info!("[db] ==> batch activate {} strategies", ids.len());
        let mut count = 0;
        for id in ids {
            let r = sqlx::query(
                "UPDATE collection_strategy SET status = '可用', updated_at = ? WHERE id = ?",
            )
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
            count += r.rows_affected() as usize;
        }
        Ok(count)
    }

    pub async fn list_active_periodic_strategies(&self) -> Result<Vec<CollectionStrategyRow>> {
        trace_sql!("SELECT id, collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at FROM collection_strategy WHERE strategy_type = 'periodic' AND status = '可用'");
        sqlx::query_as::<_, CollectionStrategyRow>(
            "SELECT id, collector_name, collector_id, table_name, status, cron_expression, collect_interval, data_interval, data_start_time, data_end_time, execute_time, agent_ids, strategy_type, created_at, updated_at FROM collection_strategy WHERE strategy_type = 'periodic' AND status = '可用'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn task_exists_by_logical_key(&self, logical_task_key: &str) -> Result<bool> {
        trace_sql!("SELECT COUNT(*) FROM collect_tasks WHERE logical_task_key = ?", logical_task_key = logical_task_key);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collect_tasks WHERE logical_task_key = ?")
            .bind(logical_task_key)
            .fetch_one(&self.pool)
            .await?;
        Ok(count > 0)
    }

    pub async fn list_data_collector_units(&self) -> Result<Vec<DataCollectorUnitRow>> {
        trace_sql!("SELECT * FROM data_collector_unit ORDER BY id DESC");
        let rows = sqlx::query_as::<_, DataCollectorUnitRow>(
            "SELECT id, unit_name, config_name, config_version, table_names, agent_ids, \
             data_interval_seconds, collector_interval, task_timeout_seconds, \
             source_type, file_encoding, remote_pattern, host, port, username, password, \
             connect_retry, download_retry, download_parallel, retry_interval_secs, \
             connect_timeout_secs, read_timeout_secs, cache_retention_days, \
             load_type, output_delimiter, db_host, db_port, db_user, db_password, \
             db_database, db_table_name_case, \
             created_at, updated_at \
             FROM data_collector_unit ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        let rows = rows.into_iter().map(|mut r| {
            r.password = "******".to_string();
            r
        }).collect();
        Ok(rows)
    }

    pub async fn get_unit_by_id(&self, id: i64) -> Result<Option<DataCollectorUnitRow>> {
        trace_sql!("SELECT * FROM data_collector_unit WHERE id = ?", id = id);
        let row = sqlx::query_as::<_, DataCollectorUnitRow>(
            "SELECT id, unit_name, config_name, config_version, table_names, agent_ids, data_interval_seconds, collector_interval, task_timeout_seconds, source_type, file_encoding, remote_pattern, host, port, username, password, connect_retry, download_retry, download_parallel, retry_interval_secs, connect_timeout_secs, read_timeout_secs, cache_retention_days, load_type, output_delimiter, db_host, db_port, db_user, db_password, db_database, db_table_name_case, created_at, updated_at FROM data_collector_unit WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn upsert_data_collector_unit(
        &self,
        id: i64,
        data: &DataCollectorUnitSaveRequest,
    ) -> Result<()> {
        trace_sql!("SELECT COUNT(*) FROM config_snapshots WHERE name = ? AND is_active = 1", config_name = data.config_name);
        let config_exists: bool = sqlx::query_scalar::<_, i32>(
            "SELECT COUNT(*) FROM config_snapshots WHERE name = ? AND is_active = 1",
        )
        .bind(&data.config_name)
        .fetch_one(&self.pool)
        .await? != 0;
        if !config_exists {
            anyhow::bail!("config_name '{}' not found or not active", data.config_name);
        }

        let agent_ids: Vec<String> = serde_json::from_str::<Vec<serde_json::Value>>(&data.agent_ids)
            .map_err(|_| anyhow::anyhow!("agent_ids is not a valid JSON array"))?
            .into_iter()
            .map(|v| match v {
                serde_json::Value::String(s) => Ok(s),
                serde_json::Value::Number(n) => Ok(n.to_string()),
                _ => anyhow::bail!("agent_ids contains invalid value: {v:?}"),
            })
            .collect::<Result<Vec<_>>>()?;
        for aid in &agent_ids {
            trace_sql!("SELECT COUNT(*) FROM agent_info WHERE agent_id = ?", agent_id = aid);
            let agent_exists: bool = sqlx::query_scalar::<_, i32>(
                "SELECT COUNT(*) FROM agent_info WHERE agent_id = ?",
            )
            .bind(aid)
            .fetch_one(&self.pool)
            .await? != 0;
            if !agent_exists {
                trace_sql!("SELECT COUNT(*) FROM agent_group WHERE group_id = ?", group_id = aid);
                let group_exists: bool = sqlx::query_scalar::<_, i32>(
                    "SELECT COUNT(*) FROM agent_group WHERE group_id = ?",
                )
                .bind(aid)
                .fetch_one(&self.pool)
                .await? != 0;
                if !group_exists {
                    anyhow::bail!("agent_id '{}' not found", aid);
                }
            }
        }

        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let password = match &data.password {
            Some(p) if p.is_empty() || p == "******" => {
                trace_sql!("SELECT password FROM data_collector_unit WHERE id = ?", id = id);
                let existing: String = sqlx::query_scalar::<_, String>(
                    "SELECT password FROM data_collector_unit WHERE id = ?",
                )
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .unwrap_or_default();
                existing
            }
            Some(p) => p.clone(),
            None => String::new(),
        };

        let db_password = match &data.db_password {
            Some(p) if p.is_empty() || p == "******" => {
                trace_sql!("SELECT db_password FROM data_collector_unit WHERE id = ?", id = id);
                let existing: String = sqlx::query_scalar::<_, String>(
                    "SELECT db_password FROM data_collector_unit WHERE id = ?",
                )
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .unwrap_or_default();
                existing
            }
            Some(p) => p.clone(),
            None => String::new(),
        };

        trace_sql!("SELECT config_snapshot_id FROM config_snapshots WHERE name = ? AND is_active = 1 ORDER BY created_at DESC LIMIT 1", config_name = data.config_name);
        let config_version: String = sqlx::query_scalar(
            "SELECT config_snapshot_id FROM config_snapshots WHERE name = ? AND is_active = 1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&data.config_name)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or_default();

        trace_sql!("SELECT created_at FROM data_collector_unit WHERE id = ?", id = id);
        let existing_created: Option<String> = sqlx::query_scalar(
            "SELECT created_at FROM data_collector_unit WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let created_at = existing_created.unwrap_or_else(|| now.clone());

        trace_sql!("INSERT OR REPLACE INTO data_collector_unit(...) VALUES(?)", id = id, unit_name = data.unit_name, config_name = data.config_name);
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO data_collector_unit(
                id, unit_name, config_name, config_version, table_names, agent_ids,
                data_interval_seconds, collector_interval, task_timeout_seconds,
                source_type, file_encoding, remote_pattern, host, port, username, password,
                connect_retry, download_retry, download_parallel, retry_interval_secs,
                connect_timeout_secs, read_timeout_secs, cache_retention_days,
                load_type, output_delimiter, db_host, db_port, db_user, db_password,
                db_database, db_table_name_case,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(&data.unit_name)
        .bind(&data.config_name)
        .bind(&config_version)
        .bind(&data.table_names)
        .bind(&data.agent_ids)
        .bind(data.data_interval_seconds.unwrap_or(900))
        .bind(data.collector_interval.unwrap_or(900))
        .bind(data.task_timeout_seconds.unwrap_or(3600))
        .bind(data.source_type.as_deref().unwrap_or("sftp"))
        .bind(data.file_encoding.as_deref().unwrap_or("UTF-8"))
        .bind(data.remote_pattern.as_deref().unwrap_or(""))
        .bind(data.host.as_deref().unwrap_or(""))
        .bind(data.port.unwrap_or(22))
        .bind(data.username.as_deref().unwrap_or(""))
        .bind(&password)
        .bind(data.connect_retry.unwrap_or(3))
        .bind(data.download_retry.unwrap_or(3))
        .bind(data.download_parallel.unwrap_or(4))
        .bind(data.retry_interval_secs.unwrap_or(30))
        .bind(data.connect_timeout_secs.unwrap_or(30))
        .bind(data.read_timeout_secs.unwrap_or(300))
        .bind(data.cache_retention_days.unwrap_or(7))
        .bind(data.load_type.as_deref().unwrap_or("clickhouse"))
        .bind(data.output_delimiter.as_deref().unwrap_or("|"))
        .bind(data.db_host.as_deref().unwrap_or(""))
        .bind(data.db_port.unwrap_or(9000))
        .bind(data.db_user.as_deref().unwrap_or(""))
        .bind(&db_password)
        .bind(data.db_database.as_deref().unwrap_or(""))
        .bind(data.db_table_name_case.as_deref().unwrap_or("lower"))
        .bind(&created_at)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_data_collector_unit(&self, id: i64) -> Result<bool> {
        trace_sql!("DELETE FROM data_collector_unit WHERE id=?", id = id);
        let result = sqlx::query("DELETE FROM data_collector_unit WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn search_active_config_names(&self, search: Option<&str>) -> Result<Vec<ConfigNameItem>> {
        match search {
            Some(q) if !q.is_empty() => {
                let pattern = format!("%{}%", q);
                trace_sql!("SELECT DISTINCT name,config_snapshot_id FROM config_snapshots WHERE is_active=1 AND name LIKE ? ORDER BY name", search = q);
                let rows = sqlx::query_as::<_, ConfigNameItem>(
                    "SELECT DISTINCT name, config_snapshot_id AS version FROM config_snapshots WHERE is_active = 1 AND name LIKE ? ORDER BY name",
                )
                .bind(&pattern)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows)
            }
            _ => {
                trace_sql!("SELECT DISTINCT name,config_snapshot_id FROM config_snapshots WHERE is_active=1 ORDER BY name");
                let rows = sqlx::query_as::<_, ConfigNameItem>(
                    "SELECT DISTINCT name, config_snapshot_id AS version FROM config_snapshots WHERE is_active = 1 ORDER BY name",
                )
                .fetch_all(&self.pool)
                .await?;
                Ok(rows)
            }
        }
    }

    pub async fn get_active_snapshot_id_for_config_name(&self, config_name: &str) -> Result<Option<String>> {
        trace_sql!("SELECT config_snapshot_id FROM config_snapshots WHERE name = ? AND is_active = 1 ORDER BY created_at DESC LIMIT 1", config_name = config_name);
        sqlx::query_scalar(
            "SELECT config_snapshot_id FROM config_snapshots WHERE name = ? AND is_active = 1 ORDER BY created_at DESC LIMIT 1"
        )
        .bind(config_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn upsert_agent_info(
        &self,
        agent_id: i64,
        agent_name: &str,
        agent_ip: &str,
        port: u16,
        version: &str,
        cpu_total: Option<&str>,
        memory_total: Option<f64>,
        disk_total: Option<f64>,
        max_thread_num: Option<i32>,
        fact_memory_total: Option<f64>,
        heartbeat_interval: Option<i32>,
        is_core: bool,
        agent_alias: Option<&str>,
        deploy_dir: &str,
    ) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("INSERT INTO agent_info(agent_id, agent_name, agent_ip, port, version, agent_alias, ...) ON CONFLICT(agent_id) DO UPDATE ...", agent_id = agent_id, agent_name = agent_name, agent_ip = agent_ip, port = port, version = version, agent_alias = agent_alias);
        sqlx::query(
            r#"
            INSERT INTO agent_info(agent_id, agent_name, agent_ip, port, version, cpu_total, memory_total, disk_total, max_thread_num, fact_memory_total, heartbeat_interval, is_core, agent_alias, registered_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(agent_id) DO UPDATE SET
                agent_name=excluded.agent_name,
                agent_ip=excluded.agent_ip,
                port=excluded.port,
                version=excluded.version,
                cpu_total=COALESCE(excluded.cpu_total, agent_info.cpu_total),
                memory_total=COALESCE(excluded.memory_total, agent_info.memory_total),
                disk_total=COALESCE(excluded.disk_total, agent_info.disk_total),
                max_thread_num=COALESCE(excluded.max_thread_num, agent_info.max_thread_num),
                fact_memory_total=COALESCE(excluded.fact_memory_total, agent_info.fact_memory_total),
                heartbeat_interval=COALESCE(excluded.heartbeat_interval, agent_info.heartbeat_interval),
                is_core=excluded.is_core,
                agent_alias=COALESCE(excluded.agent_alias, agent_info.agent_alias),
                registered_at=excluded.registered_at
            "#,
        )
        .bind(agent_id)
        .bind(agent_name)
        .bind(agent_ip)
        .bind(port as i32)
        .bind(version)
        .bind(cpu_total)
        .bind(memory_total)
        .bind(disk_total)
        .bind(max_thread_num)
        .bind(fact_memory_total)
        .bind(heartbeat_interval)
        .bind(is_core as i32)
        .bind(agent_alias)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 计算 agent 别名：{ip第三段}.{ip第四段}.{该IP已有agent数+1:02d}
    pub async fn compute_alias(&self, agent_ip: &str) -> Option<String> {
        let parts: Vec<&str> = agent_ip.split('.').collect();
        if parts.len() != 4 { return None; }
        let prefix = format!("{}.{}", parts[2], parts[3]);
        trace_sql!("SELECT COUNT(DISTINCT agent_name) FROM agent_info WHERE agent_ip = ?", agent_ip = agent_ip);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT agent_name) FROM agent_info WHERE agent_ip = ?")
            .bind(agent_ip)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        Some(format!("{}.{:02}", prefix, count + 1))
    }

    /// 查询已有别名，避免重启时重新生成
    pub async fn get_agent_alias(&self, agent_id: i64) -> Option<String> {
        trace_sql!("SELECT agent_alias FROM agent_info WHERE agent_id = ?", agent_id = agent_id);
        sqlx::query_scalar::<_, String>("SELECT agent_alias FROM agent_info WHERE agent_id = ?")
            .bind(agent_id)
            .fetch_optional(&self.pool)
            .await
            .ok()?
            .filter(|s| !s.is_empty())
    }

    pub async fn update_agent_heartbeat(
        &self,
        agent_id: i64,
        status: &str,
        cpu_load: Option<f64>,
        memory_load: Option<f64>,
        disk_load: Option<f64>,
        thread_num: Option<i32>,
    ) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("UPDATE agent_status SET status=?, cpu_load=?, memory_load=?, disk_load=?, thread_num=?, heartbeat_time=? WHERE agent_id=?", agent_id = agent_id, status = status, cpu_load = cpu_load, memory_load = memory_load, disk_load = disk_load, thread_num = thread_num);
        sqlx::query(
            r#"
            UPDATE agent_status SET status=?, cpu_load=?, memory_load=?, disk_load=?, thread_num=?, heartbeat_time=?
            WHERE agent_id=?
            "#,
        )
        .bind(status)
        .bind(cpu_load)
        .bind(memory_load)
        .bind(disk_load)
        .bind(thread_num)
        .bind(&now)
        .bind(agent_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_status_his(
        &self,
        agent_id: i64,
        cpu_load: Option<f64>,
        memory_load: Option<f64>,
        disk_load: Option<f64>,
        thread_num: Option<i32>,
    ) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("INSERT INTO agent_status_his(agent_id, cpu_load, memory_load, disk_load, thread_num, heartbeat_time) VALUES (?, ?, ?, ?, ?, ?)", agent_id = agent_id, cpu_load = cpu_load, memory_load = memory_load, disk_load = disk_load, thread_num = thread_num);
        sqlx::query(
            r#"
            INSERT INTO agent_status_his(agent_id, cpu_load, memory_load, disk_load, thread_num, heartbeat_time)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(agent_id)
        .bind(cpu_load)
        .bind(memory_load)
        .bind(disk_load)
        .bind(thread_num)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_agent_offline(&self, agent_id: i64) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("UPDATE agent_status SET status='OFFLINE', heartbeat_time=? WHERE agent_id=?", agent_id = agent_id);
        sqlx::query(
            "UPDATE agent_status SET status='OFFLINE', heartbeat_time=? WHERE agent_id=?",
        )
        .bind(&now)
        .bind(agent_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_agent_status(&self, agent_id: i64, status: &str) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("INSERT INTO agent_status(agent_id, status, heartbeat_time) VALUES (?, ?, ?) ON CONFLICT(agent_id) DO UPDATE SET status=excluded.status, heartbeat_time=excluded.heartbeat_time", agent_id = agent_id, status = status);
        sqlx::query(
            r#"
            INSERT INTO agent_status(agent_id, status, heartbeat_time)
            VALUES (?, ?, ?)
            ON CONFLICT(agent_id) DO UPDATE SET
                status=excluded.status,
                heartbeat_time=excluded.heartbeat_time
            "#,
        )
        .bind(agent_id)
        .bind(status)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_agents_with_status(&self) -> Result<Vec<AgentInfoRow>> {
        trace_sql!("SELECT ai.*, ast.* FROM agent_info ai LEFT JOIN agent_status ast ON ast.agent_id = ai.agent_id ORDER BY ai.time_stamp DESC");
        sqlx::query_as::<_, AgentInfoRow>(
            r#"
            SELECT ai.*, ast.status as current_status, ast.cpu_load, ast.memory_load, ast.disk_load,
                   ast.thread_num as current_thread_num, ast.heartbeat_time as last_heartbeat_time
            FROM agent_info ai
            LEFT JOIN agent_status ast ON ast.agent_id = ai.agent_id
            ORDER BY ai.time_stamp DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("list agents: {e}"))
    }

    pub async fn get_agent_detail(&self, agent_id: i64) -> Result<Option<AgentInfoRow>> {
        trace_sql!("SELECT ai.*, ast.* FROM agent_info ai LEFT JOIN agent_status ast ON ast.agent_id = ai.agent_id WHERE ai.agent_id = ?", agent_id = agent_id);
        sqlx::query_as::<_, AgentInfoRow>(
            r#"
            SELECT ai.*, ast.status as current_status, ast.cpu_load, ast.memory_load, ast.disk_load,
                   ast.thread_num as current_thread_num, ast.heartbeat_time as last_heartbeat_time
            FROM agent_info ai
            LEFT JOIN agent_status ast ON ast.agent_id = ai.agent_id
            WHERE ai.agent_id = ?
            "#,
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("get agent detail: {e}"))
    }

    pub async fn update_agent_info(
        &self,
        agent_id: i64,
        alias: Option<&str>,
        isuse_flag: Option<i32>,
        power: Option<f64>,
        load_limit: Option<f64>,
        description: Option<&str>,
    ) -> Result<()> {
        let mut sql = String::from("UPDATE agent_info SET");
        let mut set_parts: Vec<String> = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if let Some(v) = alias {
            set_parts.push(" agent_alias = ?".to_string());
            params.push(v.to_string());
        }
        if let Some(v) = isuse_flag {
            set_parts.push(" agent_isuse_flag = ?".to_string());
            params.push(v.to_string());
        }
        if let Some(v) = power {
            set_parts.push(" agent_power = ?".to_string());
            params.push(v.to_string());
        }
        if let Some(v) = load_limit {
            set_parts.push(" host_load_limit = ?".to_string());
            params.push(v.to_string());
        }
        if let Some(v) = description {
            set_parts.push(" description = ?".to_string());
            params.push(v.to_string());
        }

        if set_parts.is_empty() {
            return Ok(());
        }

        sql.push_str(&set_parts.join(","));
        sql.push_str(" WHERE agent_id = ?");
        params.push(agent_id.to_string());
        tracing::info!("[db] ==> {}  Parameters: {:?}", sql, params);

        let mut query = sqlx::query(&sql);
        for p in params {
            query = query.bind(p);
        }
        query.execute(&self.pool).await?;
        Ok(())
    }

    pub async fn list_agent_status(&self) -> Result<Vec<AgentStatusRow>> {
        trace_sql!("SELECT ast.*, ai.agent_name, ai.agent_alias FROM agent_status ast JOIN agent_info ai ON ai.agent_id = ast.agent_id WHERE ai.agent_isuse_flag = 1 ORDER BY ast.heartbeat_time DESC");
        sqlx::query_as::<_, AgentStatusRow>(
            r#"
            SELECT ast.*, ai.agent_name, ai.agent_alias
            FROM agent_status ast
            JOIN agent_info ai ON ai.agent_id = ast.agent_id
            WHERE ai.agent_isuse_flag = 1
            ORDER BY ast.heartbeat_time DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("list status: {e}"))
    }

    pub async fn get_status_history(&self, agent_id: i64, limit: i32) -> Result<Vec<AgentStatusHisRow>> {
        trace_sql!("SELECT * FROM agent_status_his WHERE agent_id = ? ORDER BY heartbeat_time DESC LIMIT ?", agent_id = agent_id, limit = limit);
        sqlx::query_as::<_, AgentStatusHisRow>(
            r#"
            SELECT * FROM agent_status_his WHERE agent_id = ?
            ORDER BY heartbeat_time DESC LIMIT ?
            "#,
        )
        .bind(agent_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("status history: {e}"))
    }

    pub async fn list_agent_groups(&self) -> Result<Vec<AgentGroupRow>> {
        trace_sql!("SELECT group_id, group_name, agent_ids, description, time_stamp FROM agent_group ORDER BY time_stamp DESC");
        sqlx::query_as::<_, AgentGroupRow>(
            "SELECT group_id, group_name, agent_ids, description, time_stamp FROM agent_group ORDER BY time_stamp DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("list groups: {e}"))
    }

    pub async fn create_agent_group(&self, name: &str, agent_ids: &str, description: Option<&str>) -> Result<i64> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let group_id = crate::crc64::crc64_ecma(name);
        trace_sql!("INSERT OR REPLACE INTO agent_group(group_id, group_name, agent_ids, description, time_stamp) VALUES (?, ?, ?, ?, ?)", group_id = group_id, name = name, agent_ids = agent_ids, description = description);
        sqlx::query(
            "INSERT OR REPLACE INTO agent_group(group_id, group_name, agent_ids, description, time_stamp) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(group_id)
        .bind(name)
        .bind(agent_ids)
        .bind(description)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(group_id)
    }

    pub async fn update_agent_group(&self, group_id: i64, name: &str, agent_ids: &str, description: Option<&str>) -> Result<()> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        trace_sql!("UPDATE agent_group SET group_name = ?, agent_ids = ?, description = ?, time_stamp = ? WHERE group_id = ?", group_id = group_id, name = name, agent_ids = agent_ids, description = description);
        sqlx::query(
            "UPDATE agent_group SET group_name = ?, agent_ids = ?, description = ?, time_stamp = ? WHERE group_id = ?",
        )
        .bind(name)
        .bind(agent_ids)
        .bind(description)
        .bind(&now)
        .bind(group_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_agent_group(&self, group_id: i64) -> Result<bool> {
        trace_sql!("DELETE FROM agent_group WHERE group_id = ?", group_id = group_id);
        let result = sqlx::query("DELETE FROM agent_group WHERE group_id = ?")
            .bind(group_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn tables_for_config(&self, config_name: &str) -> Result<Vec<String>> {
        trace_sql!("SELECT DISTINCT ct.table_name FROM config_tables ct INNER JOIN config_snapshots cs ON ct.config_snapshot_id = cs.config_snapshot_id WHERE cs.name = ? AND cs.is_active = 1 ORDER BY ct.table_name", config_name = config_name);
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT ct.table_name FROM config_tables ct \
             INNER JOIN config_snapshots cs ON ct.config_snapshot_id = cs.config_snapshot_id \
             WHERE cs.name = ? AND cs.is_active = 1 ORDER BY ct.table_name",
        )
        .bind(config_name)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn expand_agent_group(&self, group_id: i64) -> Result<Vec<String>> {
        trace_sql!("SELECT agent_ids FROM agent_group WHERE group_id = ?", group_id = group_id);
        let agent_ids: Option<String> = sqlx::query_scalar("SELECT agent_ids FROM agent_group WHERE group_id = ?")
            .bind(group_id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(agent_ids) = agent_ids else { return Ok(Vec::new()); };
        let ids = serde_json::from_str::<Vec<String>>(&agent_ids)
            .or_else(|_| Ok::<Vec<String>, serde_json::Error>(agent_ids.split(',').map(str::trim).filter(|s| !s.is_empty()).map(ToOwned::to_owned).collect()))?;
        Ok(ids)
    }

    pub async fn list_dispatch_candidates(&self, agent_ids: &[String]) -> Result<Vec<AgentDispatchCandidate>> {
        if agent_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat("?").take(agent_ids.len()).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT ai.agent_id, ai.agent_name, ai.agent_alias, ai.agent_isuse_flag, ai.agent_power, ai.host_load_limit, ast.status as current_status, ast.cpu_load, ast.memory_load, ast.thread_num as current_thread_num, ast.heartbeat_time as last_heartbeat_time FROM agent_info ai LEFT JOIN agent_status ast ON ast.agent_id = ai.agent_id WHERE ai.agent_id IN ({})",
            placeholders
        );
        tracing::info!("[db] ==> {}  Parameters: {:?}", sql, agent_ids);
        let mut query = sqlx::query_as::<_, AgentDispatchCandidate>(&sql);
        for agent_id in agent_ids {
            query = query.bind(agent_id.parse::<i64>().unwrap_or(0));
        }
        query.fetch_all(&self.pool).await.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use super::*;
    use crate::core::agent_id::compute_agent_id;
    use crate::core_agent_api::{RuleFile, TaskStatus};
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

    #[tokio::test]
    async fn registers_agent_and_reuses_existing_agent_id() {
        let db = db().await;
        let agent_id = compute_agent_id("127.0.0.1", "/test");
        db.upsert_agent_info(agent_id, "agent-1", "127.0.0.1", 18081, "1.0.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();
        db.upsert_agent_status(agent_id, "ONLINE").await.unwrap();
        // Re-register (upsert) should succeed without error
        db.upsert_agent_info(agent_id, "agent-1", "127.0.0.1", 18081, "1.0.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();
    }

    #[tokio::test]
    async fn stores_task_result_rows() {
        let db = db().await;
        let agent_id_i64 = compute_agent_id("127.0.0.1", "/test");
        db.upsert_agent_info(agent_id_i64, "agent-1", "127.0.0.1", 18081, "1.0.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();
        db.upsert_agent_status(agent_id_i64, "ONLINE").await.unwrap();
        let agent_id = agent_id_i64.to_string();
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
            "group_test",
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
    async fn create_task_persists_group_metadata() {
        let db = db().await;
        db.create_task(
            "task_grouped_1",
            "strategy_1:2026-07-08 10:00:00",
            "1",
            "snapshot_1",
            "2026-07-08 10:00:00",
            "collect_1",
            "agent_1",
            "group_123",
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT group_id, retry_count, next_retry_at, dispatch_error FROM collect_tasks WHERE task_id = ?",
        )
        .bind("task_grouped_1")
        .fetch_one(&db.pool)
        .await
        .unwrap();

        assert_eq!(row.get::<String, _>("group_id"), "group_123");
        assert_eq!(row.get::<i64, _>("retry_count"), 0);
        assert!(row.get::<Option<String>, _>("next_retry_at").is_none());
        assert!(row.get::<Option<String>, _>("dispatch_error").is_none());
    }

    #[tokio::test]
    async fn inserts_and_lists_config_snapshots() {
        let db = db().await;
        db.insert_config_snapshot_meta("v1", "sha256:aaa", "v1_label", 5, "test-name", &["t1".to_string()])
            .await
            .unwrap();
        db.insert_config_snapshot_meta("v2", "sha256:bbb", "v2_label", 3, "test-name-2", &["t2".to_string()])
            .await
            .unwrap();
        let list = db.list_config_snapshots().await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].config_snapshot_id, "v2");
    }

    #[tokio::test]
    async fn lists_online_agents() {
        let db = db().await;
        let agent_id = compute_agent_id("127.0.0.1", "/test");
        db.upsert_agent_info(agent_id, "agent-1", "127.0.0.1", 18081, "1.0.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();
        db.upsert_agent_status(agent_id, "ONLINE").await.unwrap();
        let agents = db.list_agents_with_status().await.unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, agent_id);
        assert_eq!(agents[0].agent_ip, "127.0.0.1");
        assert_eq!(agents[0].port, 18081);
        assert_eq!(agents[0].current_status.as_deref(), Some("ONLINE"));
    }

    #[tokio::test]
    async fn activate_switches_snapshot_is_active() {
        let db = db().await;
        db.insert_config_snapshot_meta("v1", "sha256:aaa", "v1_label", 5, "test", &[]).await.unwrap();
        db.insert_config_snapshot_meta("v2", "sha256:bbb", "v2_label", 3, "test2", &[]).await.unwrap();
        let meta = db.activate_config_snapshot("v1").await.unwrap();
        assert!(meta.is_active);
        let v1 = db.get_config_snapshot("v1").await.unwrap().unwrap();
        assert!(v1.is_active);
        let v2 = db.get_config_snapshot("v2").await.unwrap().unwrap();
        assert!(!v2.is_active);
    }

    #[tokio::test]
    async fn data_collector_unit_crud() {
        let db = db().await;
        let expected_id = crate::crc64::crc64_ecma("test-unit");

        let id = db.next_unit_id().await.unwrap();
        assert_eq!(id, 1);

        let save = DataCollectorUnitSaveRequest {
            unit_name: "test-unit".to_string(),
            config_name: "test-config".to_string(),
            table_names: "[\"t1\"]".to_string(),
            agent_ids: "[]".to_string(),
            data_interval_seconds: Some(900),
            collector_interval: Some(900),
            task_timeout_seconds: Some(3600),
            source_type: Some("sftp".to_string()),
            file_encoding: Some("UTF-8".to_string()),
            remote_pattern: Some("/path/{scan_start_time}".to_string()),
            host: Some("192.168.1.1".to_string()),
            port: Some(22),
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            connect_retry: Some(3),
            download_retry: Some(3),
            download_parallel: Some(4),
            retry_interval_secs: Some(30),
            connect_timeout_secs: Some(30),
            read_timeout_secs: Some(300),
            cache_retention_days: Some(7),
            load_type: None,
            output_delimiter: None,
            db_host: None,
            db_port: None,
            db_user: None,
            db_password: None,
            db_database: None,
            db_table_name_case: None,
        };
        let result = db.upsert_data_collector_unit(expected_id, &save).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found or not active"));

        db.insert_config_snapshot(&ConfigSnapshotResponse {
            config_snapshot_id: "v_test".to_string(),
            content_hash: "sha256:test".to_string(),
            source_toml: "".to_string(),
            mapping_dx_ini: "".to_string(),
            load_toml: "".to_string(),
            col_name_cut_config_ini: None,
            rules: vec![RuleFile {
                relative_path: "rules/a.json".to_string(),
                content: "{\"table_name\":\"t1\"}".to_string(),
            }],
        }).await.unwrap();
        db.insert_config_snapshot_meta("v_test", "sha256:test", "v_test", 1, "test-config", &["t1".to_string()]).await.unwrap();
        db.activate_config_snapshot("v_test").await.unwrap();

        db.upsert_data_collector_unit(expected_id, &save).await.unwrap();

        let list = db.list_data_collector_units().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].unit_name, "test-unit");
        assert_eq!(list[0].id, expected_id);
        assert_eq!(list[0].password, "******");

        let deleted = db.delete_data_collector_unit(expected_id).await.unwrap();
        assert!(deleted);
        let list = db.list_data_collector_units().await.unwrap();
        assert_eq!(list.len(), 0);

        let deleted = db.delete_data_collector_unit(999).await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn search_config_names_and_tables() {
        let db = db().await;

        db.insert_config_snapshot_meta("v1", "sha256:aaa", "v1", 1, "cfg-a", &["t1".to_string(), "t2".to_string()]).await.unwrap();
        db.insert_config_snapshot_meta("v2", "sha256:bbb", "v2", 1, "cfg-b", &["t3".to_string()]).await.unwrap();
        db.activate_config_snapshot("v1").await.unwrap();

        let names = db.search_active_config_names(None).await.unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name, "cfg-a");
        assert_eq!(names[0].version, "v1");

        let names = db.search_active_config_names(Some("cfg")).await.unwrap();
        assert_eq!(names.len(), 1);

        let tables = db.tables_for_config("cfg-a").await.unwrap();
        assert_eq!(tables, vec!["t1".to_string(), "t2".to_string()]);
        let tables = db.tables_for_config("cfg-b").await.unwrap();
        assert!(tables.is_empty());
    }

    #[tokio::test]
    async fn test_agent_tables_exist() {
        let db = CoreDb::open(":memory:").await.unwrap();
        // Verify agent_info exists
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_info")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(row.0, 0);
        // Verify agent_status exists
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_status")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(row.0, 0);
        // Verify agent_status_his exists
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_status_his")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(row.0, 0);
        // Verify agent_group exists
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_group")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(row.0, 0);
        // Verify old agents table is gone (expect error)
        let err = sqlx::query("SELECT COUNT(*) FROM agents")
            .fetch_one(&db.pool).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn test_upsert_agent_info() {
        let db = CoreDb::open(":memory:").await.unwrap();
        let id = compute_agent_id("10.0.0.1", "/test");

        db.upsert_agent_info(id, "agent-01", "10.0.0.1", 9997, "1.0.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();

        let row: (String,) = sqlx::query_as("SELECT agent_name FROM agent_info WHERE agent_id = ?")
            .bind(id).fetch_one(&db.pool).await.unwrap();
        assert_eq!(row.0, "agent-01");

        db.upsert_agent_info(id, "agent-01-v2", "10.0.0.1", 9997, "1.0.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();

        let row: (String,) = sqlx::query_as("SELECT agent_name FROM agent_info WHERE agent_id = ?")
            .bind(id).fetch_one(&db.pool).await.unwrap();
        assert_eq!(row.0, "agent-01-v2");
    }

    #[tokio::test]
    async fn test_upsert_agent_status() {
        let db = CoreDb::open(":memory:").await.unwrap();
        let id = compute_agent_id("10.0.0.1", "/test");

        db.upsert_agent_status(id, "ONLINE").await.unwrap();

        let row: (String,) = sqlx::query_as("SELECT status FROM agent_status WHERE agent_id = ?")
            .bind(id).fetch_one(&db.pool).await.unwrap();
        assert_eq!(row.0, "ONLINE");

        db.upsert_agent_status(id, "ONLINE").await.unwrap();
    }

    #[tokio::test]
    async fn collection_strategy_crud() {
        let db = db().await;

        db.insert_config_snapshot_meta("v_strat", "sha256:strat", "v_strat", 1, "cfg-strat", &["t1".to_string(), "t2".to_string()]).await.unwrap();
        db.activate_config_snapshot("v_strat").await.unwrap();

        let save = DataCollectorUnitSaveRequest {
            unit_name: "strat-unit".to_string(),
            config_name: "cfg-strat".to_string(),
            table_names: "[\"t1\",\"t2\"]".to_string(),
            agent_ids: "[]".to_string(),
            data_interval_seconds: Some(900),
            collector_interval: Some(900),
            task_timeout_seconds: Some(3600),
            source_type: Some("sftp".to_string()),
            file_encoding: Some("UTF-8".to_string()),
            remote_pattern: Some("/path".to_string()),
            host: Some("host".to_string()),
            port: Some(22),
            username: Some("u".to_string()),
            password: Some("p".to_string()),
            connect_retry: Some(3),
            download_retry: Some(3),
            download_parallel: Some(1),
            retry_interval_secs: Some(30),
            connect_timeout_secs: Some(30),
            read_timeout_secs: Some(300),
            cache_retention_days: Some(7),
            load_type: None,
            output_delimiter: None,
            db_host: None,
            db_port: None,
            db_user: None,
            db_password: None,
            db_database: None,
            db_table_name_case: None,
        };
        db.upsert_data_collector_unit(1, &save).await.unwrap();

        let req = CollectionStrategyCreateRequest {
            collector_id: 1,
            collector_name: "strat-unit".to_string(),
            table_names: vec!["t1".to_string(), "t2".to_string()],
            cron_expression: Some("0 0 * * *".to_string()),
            collect_interval: 900,
            data_interval: 900,
            data_start_time: None,
            data_end_time: None,
            execute_time: None,
            agent_ids: "[]".to_string(),
            strategy_type: "periodic".to_string(),
        };
        let rows = db.create_strategies(&req).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].table_name, "t1");
        assert_eq!(rows[0].status, "可用");

        let list = db.list_strategies(None, None, None).await.unwrap();
        assert_eq!(list.len(), 2);

        let row = db.get_strategy(rows[0].id).await.unwrap().unwrap();
        assert_eq!(row.table_name, "t1");

        let update = CollectionStrategyUpdateRequest {
            cron_expression: Some("0 */2 * * *".to_string()),
            collect_interval: None,
            data_interval: None,
            data_start_time: None,
            data_end_time: None,
            execute_time: None,
            agent_ids: None,
            status: Some("挂起".to_string()),
        };
        let ok = db.update_strategy(rows[0].id, &update).await.unwrap();
        assert!(ok);
        let updated = db.get_strategy(rows[0].id).await.unwrap().unwrap();
        assert_eq!(updated.status, "挂起");
        assert_eq!(updated.cron_expression, "0 */2 * * *");

        let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
        db.batch_suspend(&ids).await.unwrap();
        assert_eq!(db.get_strategy(rows[0].id).await.unwrap().unwrap().status, "挂起");
        assert_eq!(db.get_strategy(rows[1].id).await.unwrap().unwrap().status, "挂起");

        db.batch_activate(&ids).await.unwrap();
        assert_eq!(db.get_strategy(rows[0].id).await.unwrap().unwrap().status, "可用");
        assert_eq!(db.get_strategy(rows[1].id).await.unwrap().unwrap().status, "可用");
    }

    #[tokio::test]
    async fn test_update_agent_heartbeat() {
        let db = CoreDb::open(":memory:").await.unwrap();
        let id = compute_agent_id("10.0.0.1", "/test");
        db.upsert_agent_info(id, "a1", "10.0.0.1", 9997, "1.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();
        db.upsert_agent_status(id, "ONLINE").await.unwrap();

        db.update_agent_heartbeat(id, "ONLINE", Some(45.5), Some(60.0), Some(30.0), Some(8)).await.unwrap();

        let row: (String, Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT status, cpu_load, memory_load FROM agent_status WHERE agent_id = ?"
        ).bind(id).fetch_one(&db.pool).await.unwrap();
        assert_eq!(row.0, "ONLINE");
        assert!((row.1.unwrap() - 45.5).abs() < 0.01);
        assert!((row.2.unwrap() - 60.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_insert_status_his() {
        let db = CoreDb::open(":memory:").await.unwrap();
        let id = compute_agent_id("10.0.0.1", "/test");
        db.upsert_agent_info(id, "a1", "10.0.0.1", 9997, "1.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();

        db.insert_status_his(id, Some(50.0), Some(70.0), Some(20.0), Some(5)).await.unwrap();
        db.insert_status_his(id, Some(60.0), Some(65.0), Some(25.0), Some(6)).await.unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_status_his WHERE agent_id = ?")
            .bind(id).fetch_one(&db.pool).await.unwrap();
        assert_eq!(count.0, 2);
    }

    #[tokio::test]
    async fn test_mark_agent_offline() {
        let db = CoreDb::open(":memory:").await.unwrap();
        let id = compute_agent_id("10.0.0.1", "/test");
        db.upsert_agent_info(id, "a1", "10.0.0.1", 9997, "1.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();
        db.upsert_agent_status(id, "ONLINE").await.unwrap();

        db.mark_agent_offline(id).await.unwrap();

        let status: String = sqlx::query_scalar("SELECT status FROM agent_status WHERE agent_id = ?")
            .bind(id).fetch_one(&db.pool).await.unwrap();
        assert_eq!(status, "OFFLINE");
    }

    #[tokio::test]
    async fn test_select_online_agent() {
        let db = CoreDb::open(":memory:").await.unwrap();
        let id1 = compute_agent_id("10.0.0.1", "/test");
        let id2 = compute_agent_id("10.0.0.2", "/test");

        db.upsert_agent_info(id1, "a1", "10.0.0.1", 9997, "1.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();
        db.upsert_agent_status(id1, "ONLINE").await.unwrap();

        db.upsert_agent_info(id2, "a2", "10.0.0.2", 9997, "1.0", None, None, None, None, None, None, false, None, "/test").await.unwrap();
        db.upsert_agent_status(id2, "ONLINE").await.unwrap();

        let (aid, power) = db.select_online_agent().await.unwrap();
        assert!(aid == id1 || aid == id2);
        assert!((power - 1.0).abs() < 0.01);

        sqlx::query("UPDATE agent_info SET agent_isuse_flag=0 WHERE agent_id=?")
            .bind(id1).execute(&db.pool).await.unwrap();
        sqlx::query("UPDATE agent_status SET heartbeat_time='2020-01-01 00:00:00' WHERE agent_id=?")
            .bind(id2).execute(&db.pool).await.unwrap();

        let (aid, _) = db.select_online_agent().await.unwrap();
        assert_eq!(aid, id2);

        sqlx::query("UPDATE agent_info SET agent_isuse_flag=0 WHERE agent_id=?")
            .bind(id2).execute(&db.pool).await.unwrap();

        let result = db.select_online_agent().await;
        assert!(result.is_err(), "no online+enabled agent should return error");
    }

    #[tokio::test]
    async fn group_status_and_retry_updates_all_non_terminal_tasks() {
        let db = db().await;
        for task_id in ["task_g1_a", "task_g1_b"] {
            db.create_task(
                task_id,
                task_id,
                "1",
                "snapshot_1",
                "2026-07-08 10:00:00",
                task_id,
                "agent_old",
                "group_g1",
            )
            .await
            .unwrap();
        }

        let assigned = db.assign_group_to_agent("group_g1", "agent_new").await.unwrap();
        assert_eq!(assigned, 2);

        let updated = db.update_group_status("group_g1", "DISPATCHING", None).await.unwrap();
        assert_eq!(updated, 2);

        let retried = db
            .increment_group_retry("group_g1", "2026-07-08 10:01:00", "no available agent")
            .await
            .unwrap();
        assert_eq!(retried, 2);

        let row = sqlx::query(
            "SELECT assigned_agent_id, status, retry_count, next_retry_at, dispatch_error FROM collect_tasks WHERE task_id = ?",
        )
        .bind("task_g1_a")
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("assigned_agent_id"), "agent_new");
        assert_eq!(row.get::<String, _>("status"), "DISPATCHING");
        assert_eq!(row.get::<i64, _>("retry_count"), 1);
        assert_eq!(row.get::<String, _>("next_retry_at"), "2026-07-08 10:01:00");
        assert_eq!(row.get::<String, _>("dispatch_error"), "no available agent");
    }

    #[tokio::test]
    async fn expand_agent_group_returns_member_ids() {
        let db = db().await;
        let group_id = crate::crc64::crc64_ecma("dispatch-group");
        sqlx::query("INSERT INTO agent_group(group_id, group_name, agent_ids, time_stamp) VALUES (?, ?, ?, ?)")
            .bind(group_id)
            .bind("dispatch-group")
            .bind("[\"11\",\"22\"]")
            .bind("2026-07-08 10:00:00")
            .execute(&db.pool)
            .await
            .unwrap();

        let ids = db.expand_agent_group(group_id).await.unwrap();
        assert_eq!(ids, vec!["11".to_string(), "22".to_string()]);
    }

    #[tokio::test]
    async fn active_task_count_and_agent_failure_ignore_terminal_tasks() {
        let db = db().await;
        for (task_id, status) in [("task_active_1", "CREATED"), ("task_active_2", "RUNNING"), ("task_done", "SUCCEEDED")] {
            db.create_task(
                task_id,
                task_id,
                "1",
                "snapshot_1",
                "2026-07-08 10:00:00",
                task_id,
                "agent_1",
                "group_active",
            )
            .await
            .unwrap();
            sqlx::query("UPDATE collect_tasks SET status = ? WHERE task_id = ?")
                .bind(status)
                .bind(task_id)
                .execute(&db.pool)
                .await
                .unwrap();
        }

        assert_eq!(db.count_active_tasks_by_agent("agent_1").await.unwrap(), 2);

        let failed = db
            .mark_active_tasks_failed_for_agent("agent_1", "agent heartbeat timeout")
            .await
            .unwrap();
        assert_eq!(failed, 2);
        assert_eq!(db.count_active_tasks_by_agent("agent_1").await.unwrap(), 0);
    }
}
