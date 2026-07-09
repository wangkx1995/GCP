# OP 表采集状态记录 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Record each OP table's collection status as separate rows in result.csv and collect_tasks.

**Architecture:** Agent-side: track per-OP-table row counts during TPD engine processing; after all files parsed, write OP rows to result.csv; after TPD finish, append TPD row. Core-side: UPDATE existing TPD row, INSERT new OP rows in collect_tasks.

**Tech Stack:** Rust, sqlx, csv crate

## Global Constraints

- Keep `ResultRow` (5 fields) for existing SQL grid query; new `CsvResultRow` (8 fields) everywhere else
- `task_id` for OP rows: `{TPD_task_id}_{index}` where index = 0,1,2... sorted by OP table name alphabetically
- OP rows in collect_tasks share `group_id` and `strategy_id` with parent TPD row
- Multiple PM CSV files → same OP table → one row with cumulative `row_count`
- Grid query needs no change (stategy_id filter naturally returns all rows)

---

### Task 1: `CsvResultRow` type + thread task metadata through parse job

**Files:**
- Modify: `src/core_agent_api.rs` — add `CsvResultRow`, update `TaskResultReport`
- Modify: `src/parse_job.rs` — add fields to `ParseJobOptions` and `StreamingTableTask`
- Modify: `src/agent/runner.rs` — populate new fields

- [ ] **Step 1: Add `CsvResultRow` in `core_agent_api.rs`** after `ResultRow`:

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CsvResultRow {
    pub table_name: String,
    pub data_time: String,
    pub row_count: u64,
    pub success: i32,
    pub collect_time: String,
    pub task_id: String,
    pub strategy_id: String,
    pub group_id: String,
}
```

- [ ] **Step 2: Update `TaskResultReport`** to use `Vec<CsvResultRow>`:

```rust
pub struct TaskResultReport {
    pub task_id: String,
    pub agent_id: String,
    pub status: TaskStatus,
    pub result_rows: Vec<CsvResultRow>,
}
```

- [ ] **Step 3: Add task metadata to `ParseJobOptions`** in `parse_job.rs`:

```rust
pub struct ParseJobOptions {
    // ... existing fields unchanged ...
    pub task_id: Option<String>,
    pub strategy_id: Option<String>,
    pub group_id: Option<String>,
}
```

- [ ] **Step 4: Populate in `agent/runner.rs`** — add to the `ParseJobOptions` construction:

```rust
task_id: Some(task.task_id.clone()),
strategy_id: Some(task.strategy_id.clone()),
group_id: task.group_id.clone(),
```

- [ ] **Step 5: Add fields to `StreamingTableTask`** in `parse_job.rs` (find struct around line 323):

```rust
struct StreamingTableTask {
    dest_table: String,
    inputs: Vec<PathBuf>,
    task_id: String,
    strategy_id: String,
    group_id: String,
}
```

Update `build_streaming_table_tasks` to populate them from `opts`.

- [ ] **Step 6: Thread through `run_streaming_table_tasks`** — accept `task_id`, `strategy_id`, `group_id: &str` params and pass into each task.

- [ ] **Step 7: Compile check** — `cargo build --locked` passes.

- [ ] **Step 8: Commit:**

```bash
git add src/core_agent_api.rs src/parse_job.rs src/agent/runner.rs
git commit -m "feat: add CsvResultRow and thread task metadata through parse job"
```

---

### Task 2: Track per-OP-table row counts in `StreamingTpdEngine`

**Files:**
- Modify: `src/tpd.rs` — add counter field to `StreamingTpdEngine`

- [ ] **Step 1: Add `source_table_counts: HashMap<String, usize>` to `StreamingTpdEngine`** and init in `new()`

- [ ] **Step 2: Increment in `accept_owned`** — at the top, before dispatch:

```rust
*self.source_table_counts.entry(table.to_string()).or_insert(0) += 1;
```

- [ ] **Step 3: Increment in `accept_values`**:

```rust
*self.source_table_counts.entry(table.to_string()).or_insert(0) += 1;
```

- [ ] **Step 4: Add accessor**:

```rust
pub fn source_table_counts(&self) -> &HashMap<String, usize> { &self.source_table_counts }
```

- [ ] **Step 5: Commit:**

```bash
git add src/tpd.rs
git commit -m "feat: track per-OP-table row counts in StreamingTpdEngine"
```

---

### Task 3: 8-column write_result_csv with create/append mode + reader update

**Files:**
- Modify: `src/writer.rs` — `write_result_csv` → 8-column create/append, update `StreamingTableWriter`
- Modify: `src/agent/result_csv.rs` — parse 8 columns, update test
- Modify: `src/tpd.rs` — `StreamingFinishOptions` gains OP rows field

- [ ] **Step 1: Rewrite `write_result_csv` in `writer.rs`** to accept `&[CsvResultRow]` and optional header:

```rust
pub fn write_result_csv(path: &Path, rows: &[CsvResultRow], create: bool) -> Result<()> {
    let should_write_header = create || !path.exists();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(create)
        .append(!create)
        .truncate(create)
        .open(path)?;
    let mut writer = csv::WriterBuilder::new()
        .has_headers(should_write_header)
        .from_writer(file);
    for row in rows {
        writer.write_record([
            &row.table_name, &row.data_time, &row.row_count.to_string(),
            &row.success.to_string(), &row.collect_time,
            &row.task_id, &row.strategy_id, &row.group_id,
        ])?;
    }
    writer.flush()?;
    Ok(())
}
```

- [ ] **Step 2: Change `StreamingTableWriter::finish()`** to call `write_result_csv` in append mode.

Create the TPD `CsvResultRow` using the writer's fields (table, row_count, task_id, strategy_id, group_id):

```rust
write_result_csv(
    &result_path,
    &[CsvResultRow {
        table_name: self.table.clone(),
        data_time: package.scan_start.value.clone(),
        row_count: package.row_count,
        success: 1,
        collect_time: crate::timeutil::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        task_id: self.task_id.clone(),
        strategy_id: self.strategy_id.clone(),
        group_id: self.group_id.clone(),
    }],
    false,  // append
)?;
```

Remove the old `write_result_csv` that took 4+5 params — the old function is replaced entirely.

- [ ] **Step 3: Add task metadata to `StreamingTableWriter`** and `StreamingFinishOptions`.

Update `StreamingFinishOptions` in `tpd.rs`:
```rust
pub struct StreamingFinishOptions<'a> {
    // ... existing fields ...
    pub task_id: &'a str,
    pub strategy_id: &'a str,
    pub group_id: &'a str,
}
```

Add matching fields to `StreamingTableWriter` and populate in `new_with_headers`.

- [ ] **Step 4: Add `op_rows` to `StreamingFinishOptions`** so OP row data flows through to the writer:

```rust
pub struct StreamingFinishOptions<'a> {
    // ... existing + task_id/strategy_id/group_id ...
    pub op_rows: Vec<CsvResultRow>,
}
```

In `StreamingTableWriter::finish()`, write OP rows before TPD row:

```rust
// Write OP rows first
if !self.op_rows.is_empty() {
    write_result_csv(&result_path, &self.op_rows, true)?;  // create with header
}
// Append TPD row
write_result_csv(&result_path, &[/* TPD row */], false)?;  // append
```

Add `op_rows: Vec<CsvResultRow>` field to `StreamingTableWriter`, set in `new_with_headers`.

- [ ] **Step 5: Update `read_result_rows` in `agent/result_csv.rs`** to parse all 8 column indexes:

```rust
pub fn read_result_rows(output_dir: &Path) -> Result<Vec<CsvResultRow>> {
    // ... existing walkdir loop ...
    CsvResultRow {
        table_name: record.get(0).unwrap_or_default().to_string(),
        data_time: record.get(1).unwrap_or_default().to_string(),
        row_count: record.get(2).unwrap_or("0").parse::<u64>()?,
        success: record.get(3).unwrap_or("0").parse::<i32>()?,
        collect_time: record.get(4).unwrap_or_default().to_string(),
        task_id: record.get(5).unwrap_or_default().to_string(),
        strategy_id: record.get(6).unwrap_or_default().to_string(),
        group_id: record.get(7).unwrap_or_default().to_string(),
    }
    // ... same sort ...
}
```

- [ ] **Step 6: Update test** — add all 8 columns to the CSV fixture and assert:

```rust
std::fs::write(
    package_dir.join("result.csv"),
    "table_name,data_time,row_count,success,collect_time,task_id,strategy_id,group_id\n\
     TPD_A,2026-06-17 15:15:00,100,1,2026-07-02 15:35:00,task_123_TPD_A_20260617,strat_abc,group_xyz\n"
).unwrap();
let rows = read_result_rows(dir.path()).unwrap();
assert_eq!(rows[0].task_id, "task_123_TPD_A_20260617");
assert_eq!(rows[0].strategy_id, "strat_abc");
```

- [ ] **Step 7: Fix compilation errors** — update all `use` references:
  - `src/agent/runner.rs`: change `report_to_core` param from `Vec<ResultRow>` to `Vec<CsvResultRow>`
  - `src/writer.rs`: add `use crate::core_agent_api::CsvResultRow;`

- [ ] **Step 8: Run test to verify** — `cargo test` passes (at least the result_csv test).

- [ ] **Step 9: Commit:**

```bash
git add src/writer.rs src/agent/result_csv.rs src/tpd.rs
git commit -m "feat: 8-column result.csv with create/append modes"
```

---

### Task 4: Agent-side OP row writing in `run_streaming_table_task`

**Files:**
- Modify: `src/parse_job.rs` — write OP rows between file-parse loop and engine.finish()

- [ ] **Step 1: After file-parsing loop, collect OP counts and build `CsvResultRow` vector**:

At the end of `run_streaming_table_task`, after the `}` closing the `for input in &task.inputs` loop (line ~177):

```rust
let now = crate::timeutil::now();
let op_phase_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
let op_rows: Vec<CsvResultRow> = {
    let engine = streaming_engine.borrow();
    let counts = engine.source_table_counts();
    let mut op_tables: Vec<&String> = counts.keys().collect();
    op_tables.sort();
    op_tables.iter().enumerate().map(|(idx, name)| {
        CsvResultRow {
            table_name: (*name).clone(),
            data_time: op_phase_time.clone(),
            row_count: *counts.get(*name).unwrap_or(&0) as u64,
            success: 1,
            collect_time: op_phase_time.clone(),
            task_id: format!("{}_{}", task.task_id, idx),
            strategy_id: task.strategy_id.clone(),
            group_id: task.group_id.clone(),
        }
    }).collect()
};
```

Set these `op_rows` into the `streaming_finish_options` before calling engine.finish().

- [ ] **Step 2: Pass `op_rows` through finish options** — add field to `StreamingFinishOptions` and construct it inline:

The `streaming_finish_options` struct is constructed at line 178. After adding `op_rows`, `task_id`, `strategy_id`, `group_id` to `StreamingFinishOptions`, build it with these values.

- [ ] **Step 3: Flow from `run_parse_job` down** — ensure `run_streaming_table_tasks` and `run_streaming_table_task` all accept and pass the required params:

```rust
fn run_streaming_table_task(
    task: StreamingTableTask,
    ctx: &ContextData,
    output_dir: &Path,
    output_delimiter: u8,
    collector_name: &str,
    load_type: LoadType,
    load_config: &LoadConfig,
) -> Result<()>
```

Thread `task.task_id`, `task.strategy_id`, `task.group_id` through.

- [ ] **Step 4: Commit:**

```bash
git add src/parse_job.rs
git commit -m "feat: write OP table rows before TPD engine finish"
```

---

### Task 5: Core-side multi-row result handling

**Files:**
- Modify: `src/core/db.rs` — `accept_task_result` handles OP INSERT + TPD UPDATE

- [ ] **Step 1: Restructure `accept_task_result`** to iterate all rows.

Current logic: reads first row, destructures into 5 fields, UPDATEs single collect_tasks row.

New logic:
```
1. Verify main TPD task exists (by report.task_id), get strategy_id, group_id
2. For each row in report.result_rows:
   a. If row.task_id == report.task_id (TPD row) → UPDATE existing row
   b. Else if row.task_id starts with "{report.task_id}_" (OP row) → INSERT new row
3. Log summary
```

```rust
pub async fn accept_task_result(&self, report: &TaskResultReport) -> Result<()> {
    // ... existing verification logic ...
    let (strategy_id, config_snapshot_id, scan_start_time) = match task_row {
        Some(row) => { /* same as before, get strategy_id/scan_start_time */ }
        None => { /* synthetic creation for unknown task_id */ }
    };

    let terminal_status = match report.status { /* same */ };

    for row in &report.result_rows {
        if row.task_id == report.task_id {
            // TPD row — UPDATE existing
            sqlx::query("UPDATE collect_tasks SET status=?, finished_at=?, table_name=COALESCE(?,table_name), data_time=COALESCE(?,data_time), row_count=?, success=?, collect_time=? WHERE task_id=?")
                .bind(terminal_status).bind(&now)
                .bind(&row.table_name).bind(&row.data_time)
                .bind(row.row_count as i64).bind(row.success).bind(&row.collect_time)
                .bind(&row.task_id)
                .execute(&self.pool).await?;
        } else {
            // OP row — INSERT new
            let logical_task_key = format!("strategy_{}:{}:{}", strategy_id, row.data_time, row.table_name);
            sqlx::query(
                "INSERT OR IGNORE INTO collect_tasks(task_id, logical_task_key, strategy_id, config_snapshot_id, scan_start_time, collector_name, assigned_agent_id, status, created_at, finished_at, table_name, data_time, row_count, success, collect_time, group_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
                .bind(&row.task_id)
                .bind(&logical_task_key)
                .bind(&strategy_id)
                .bind(&config_snapshot_id)
                .bind(scan_start_time.as_deref().unwrap_or(""))
                .bind("")  // collector_name — not available here, empty is fine
                .bind(&report.agent_id)
                .bind(terminal_status)
                .bind(&now)
                .bind(&now)
                .bind(&row.table_name)
                .bind(&row.data_time)
                .bind(row.row_count as i64)
                .bind(row.success)
                .bind(&row.collect_time)
                .bind(&row.group_id)
                .execute(&self.pool).await?;
        }
    }
    Ok(())
}
```

Key decisions:
- Use `INSERT OR IGNORE` for OP rows (idempotent)
- `collector_name` left empty on OP rows (copied from TPD row if needed later)
- `group_id` stored in OP rows for query association

- [ ] **Step 2: Compile check** — `cargo build --locked` passes.

- [ ] **Step 3: Commit:**

```bash
git add src/core/db.rs
git commit -m "feat: accept_task_result handles OP row INSERT for multi-row reports"
```

---

### Task 6: Full verification

**Files:**
- All modified files above

- [ ] **Step 1: Run full test suite:**

```bash
cargo test
```

Expected: all ~62 tests pass (including `reads_nested_result_csv_rows`, `collection_strategy_crud`, and all tpd/parse_job tests).

- [ ] **Step 2: Build release and deploy to test/:**

```bash
cargo build --release --locked && \
cp target/release/core test/core && \
cp target/release/agent test/agent
```

- [ ] **Step 3: Verify existing grid query still works** — start test/core and check GET /api/results/grid returns valid data for existing TPD tasks.

- [ ] **Step 4: Commit final:**

```bash
git commit --allow-empty -m "feat: complete OP table collection status tracking"
```
