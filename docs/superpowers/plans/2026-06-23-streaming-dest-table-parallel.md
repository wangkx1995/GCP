# Streaming Dest Table Parallel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run all TPD rules through streaming-only execution and process independent destination tables with automatic per-table parallelism.

**Architecture:** `remote-file-source` returns both representative files and routed groups keyed by destination table. `main.rs` validates all rules are streaming-compatible, builds one task per destination table, and runs each task with its own `StreamingTpdEngine`, parser callbacks, and output finish. Non-streaming TPD execution is removed from the main flow.

**Tech Stack:** Rust 2021, std scoped threads, existing parser streaming values path, existing `StreamingTpdEngine`, existing `StreamingTableWriter`.

## Global Constraints

- Remove the non-streaming TPD execution path from the main pipeline.
- Treat every loaded rule as required to be streaming-compatible.
- Fail at startup when any rule cannot be represented by the streaming engine.
- Parallelize only by destination table in this phase.
- Keep each destination table single-threaded internally to avoid writer and aggregator sharing.
- Do not expose a streaming parallelism flag; effective parallelism equals destination table task count.
- Preserve local `--input` support by giving each destination table task the same local input list.
- Do not modify or rely on local secret-bearing `source.toml`.

---

### Task 1: Routed Input Groups

**Files:**
- Modify: `crates/remote-file-source/src/lib.rs`
- Modify: `crates/remote-file-source/src/remote.rs`

**Interfaces:**
- Produces: `pub struct RoutedInputs { pub representative_files: Vec<PathBuf>, pub groups: Vec<RoutedInputGroup> }`
- Produces: `pub struct RoutedInputGroup { pub route: String, pub files: Vec<PathBuf> }`
- Produces: `pub fn resolve_routed_files_with_router<F>(options: ResolveOptions, route_remote_file: F) -> Result<RoutedInputs>`
- Keeps: `resolve_files` and `resolve_files_with_router` returning representative files.

- [ ] **Step 1: Add failing route grouping tests**

Add tests to `crates/remote-file-source/src/remote.rs` that call a new pure helper `download_result_from_successes` with simulated successes:

```rust
#[test]
fn download_result_groups_successes_by_route() {
    let targets = vec![
        DownloadTarget {
            remote_index: 0,
            route_index: 0,
            route: Some("TPD_A".to_string()),
            remote_file: "/remote/a.csv.gz".to_string(),
            local_path: PathBuf::from("downloads/tpd_a/a.csv.gz"),
        },
        DownloadTarget {
            remote_index: 0,
            route_index: 1,
            route: Some("TPD_B".to_string()),
            remote_file: "/remote/a.csv.gz".to_string(),
            local_path: PathBuf::from("downloads/tpd_b/a.csv.gz"),
        },
    ];

    let result = download_result_from_successes(1, targets).unwrap();

    assert_eq!(result.representative_files, vec![PathBuf::from("downloads/tpd_a/a.csv.gz")]);
    assert_eq!(result.groups.len(), 2);
    assert_eq!(result.groups[0].route, "TPD_A");
    assert_eq!(result.groups[0].files, vec![PathBuf::from("downloads/tpd_a/a.csv.gz")]);
    assert_eq!(result.groups[1].route, "TPD_B");
    assert_eq!(result.groups[1].files, vec![PathBuf::from("downloads/tpd_b/a.csv.gz")]);
}
```

- [ ] **Step 2: Run focused tests and verify failure**

Run: `cargo test -p remote-file-source download_result_groups_successes_by_route`

Expected: FAIL because `DownloadTarget.route` and `download_result_from_successes` do not exist.

- [ ] **Step 3: Implement public grouped types and result helper**

Add public structs in `lib.rs`, add internal `DownloadResult` conversion or re-use public `RoutedInputs`, and add `route: Option<String>` to `DownloadTarget`. The helper must sort by `(remote_index, route_index)` and return one representative path per remote index.

- [ ] **Step 4: Return grouped results from remote downloads**

Change `remote::download_files_with_router` to return `RoutedInputs`. Sequential and parallel paths must collect all successful `DownloadTarget`s and pass them into `download_result_from_successes`.

- [ ] **Step 5: Preserve compatibility wrappers**

`resolve_files` and `resolve_files_with_router` should call the routed resolver and return `.representative_files`.

- [ ] **Step 6: Verify remote crate**

Run: `cargo test -p remote-file-source`

Expected: all remote-file-source tests pass.

---

### Task 2: Streaming Compatibility Validation

**Files:**
- Modify: `src/tpd.rs`

**Interfaces:**
- Produces: `pub fn validate_streaming_rules(rules: &[TpdRule]) -> Result<()>`
- Produces: `pub fn streaming_rule_tables(rules: &[TpdRule]) -> HashSet<String>` remains available.

- [ ] **Step 1: Add failing validation tests**

Add tests in `src/tpd.rs`:

```rust
#[test]
fn validate_streaming_rules_rejects_rule_without_one_enabled_group() {
    let rule: TpdRule = serde_json::from_str(
        r#"{
          "table_name":"TPD_BAD",
          "groups":[
            {"name":"g1","enabled":true,"source_table":"OP_A","group_by":["dn"]},
            {"name":"g2","enabled":true,"source_table":"OP_A","group_by":["dn"]}
          ],
          "temp_fields":[],
          "output_fields":[]
        }"#,
    ).unwrap();

    let err = validate_streaming_rules(&[rule]).unwrap_err();

    assert!(err.to_string().contains("TPD_BAD"));
    assert!(err.to_string().contains("streaming-compatible"));
}
```

- [ ] **Step 2: Run focused test and verify failure**

Run: `cargo test validate_streaming_rules_rejects_rule_without_one_enabled_group`

Expected: FAIL because `validate_streaming_rules` does not exist.

- [ ] **Step 3: Implement validation**

Implement `validate_streaming_rules` by calling a new private helper that returns an error reason when `StreamingRuleAggregator::new(rule)` returns `None`. The first reason required now is `expected exactly one enabled group`.

- [ ] **Step 4: Verify focused tests**

Run: `cargo test validate_streaming_rules_`

Expected: validation tests pass.

---

### Task 3: Streaming Table Task Construction

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Produces: `struct StreamingTableTask { dest_table: String, rules: Vec<tpd::TpdRule>, inputs: Vec<PathBuf> }`
- Produces: `fn build_streaming_table_tasks(rules: &[tpd::TpdRule], routed_groups: &[remote_file_source::RoutedInputGroup], fallback_inputs: &[PathBuf]) -> Result<Vec<StreamingTableTask>>`

- [ ] **Step 1: Add task construction tests**

Add a `#[cfg(test)]` module to `src/main.rs` with JSON rule fixtures. Test that `TPD_A` receives only `TPD_A` rules and routed group files, while local fallback inputs are used when groups are empty.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `cargo test build_streaming_table_tasks_`

Expected: FAIL because helper does not exist.

- [ ] **Step 3: Implement task construction**

Group rules by uppercase `table_name`. Match routed groups by uppercase `route`. If routed groups exist, destination tables without a matching non-empty group are skipped with an `[input] skip <table>: no routed input files` log line. If routed groups are empty, use `fallback_inputs` for every destination table.

- [ ] **Step 4: Verify focused tests**

Run: `cargo test build_streaming_table_tasks_`

Expected: PASS.

---

### Task 4: Streaming-Only Main Flow

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `remote_file_source::resolve_routed_files_with_router`
- Consumes: `tpd::validate_streaming_rules`
- Produces: `fn run_streaming_table_task(...) -> Result<()>`

- [ ] **Step 1: Replace input resolution**

Use `resolve_routed_files_with_router` and keep both `routed_inputs.representative_files` and `routed_inputs.groups`.

- [ ] **Step 2: Validate streaming rules before downloads when possible**

Call `tpd::validate_streaming_rules(&rules)?` immediately after loading rules and before remote resolution.

- [ ] **Step 3: Remove non-streaming table flow from main**

Delete main-flow uses of `TableRows`, `non_streaming_source_tables`, and `tpd::execute_tpd_rule`. Keep `TableRows` type alias if still used by `tpd.rs` through crate imports.

- [ ] **Step 4: Implement single task runner**

For one `StreamingTableTask`, build a `StreamingTpdEngine` from its rules, parse its `inputs`, accept only consumed tables, and finish with a local empty `TableRows`.

- [ ] **Step 5: Implement automatic task execution**

Run all destination table tasks with effective parallelism equal to task count. A single task still runs normally with one worker.

- [ ] **Step 6: Verify full tests**

Run: `cargo test`

Expected: all tests pass.

---

### Task 5: Destination Table Parallelism

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `run_streaming_table_task`
- Produces: `fn effective_streaming_parallelism(task_count: usize) -> usize`
- Produces: `fn run_streaming_table_tasks(tasks: Vec<StreamingTableTask>, ...) -> Result<()>`

- [ ] **Step 1: Add failing automatic parallelism test**

Add a unit test in `src/main.rs`:

```rust
#[test]
fn effective_streaming_parallelism_uses_task_count() {
    assert_eq!(effective_streaming_parallelism(0), 0);
    assert_eq!(effective_streaming_parallelism(1), 1);
    assert_eq!(effective_streaming_parallelism(3), 3);
}
```

Run: `cargo test effective_streaming_parallelism_uses_task_count`

Expected: FAIL because `effective_streaming_parallelism` does not exist.

- [ ] **Step 2: Remove CLI parameter**

Remove `streaming_parallel` from `Cli`, remove its validation, and remove it from `run_streaming_table_tasks` arguments.

- [ ] **Step 3: Implement automatic parallel runner**

Use `std::thread::scope` and spawn one scoped thread per task. Each scoped thread returns `Result<()>`; collect errors with destination table names.

- [ ] **Step 4: Add logs**

Log task count and effective parallelism:

```text
[aggregate] streaming destination tables: 5 task(s), parallel=5
```

- [ ] **Step 5: Verify full tests**

Run: `cargo test --workspace`

Expected: all tests pass.

---

### Task 6: Final Verification and Commit

**Files:**
- Modify: `crates/remote-file-source/src/lib.rs`
- Modify: `crates/remote-file-source/src/remote.rs`
- Modify: `src/main.rs`
- Modify: `src/tpd.rs`
- Create: `docs/superpowers/plans/2026-06-23-streaming-dest-table-parallel.md`

- [ ] **Step 1: Format and verify**

Run: `cargo fmt && cargo test --workspace && cargo build --release --locked`

Expected: tests and build pass without warnings.

- [ ] **Step 2: Inspect status and diff**

Run: `git status --short --branch` and inspect intended files only. Do not stage `source.toml`. Do not stage unrelated `src/parser.rs` newline-only changes unless it is already necessary for `cargo fmt` consistency.

- [ ] **Step 3: Commit locally**

Run:

```bash
git add crates/remote-file-source/src/lib.rs crates/remote-file-source/src/remote.rs src/main.rs src/tpd.rs docs/superpowers/plans/2026-06-23-streaming-dest-table-parallel.md
git commit -m "feat: run streaming tables independently"
```

- [ ] **Step 4: Report results**

Report commit hash, verification commands, and any unstaged unrelated files.
