# Dest Table Download Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Download one remote copy per destination-table directory while preserving current parser behavior by returning one representative path per matched remote file.

**Architecture:** The root crate loads mapping/rules before remote resolution and passes a router closure into `remote-file-source`. The remote crate expands each matched remote file into one or more download targets under `<download_dir>/<dest_table_lower>/`, but returns only the first successful target per remote file until parser grouping is implemented.

**Tech Stack:** Rust 2021, existing `remote-file-source` crate, current `mapping_dx.ini` filename detection, TPD rule metadata.

## Global Constraints

- Only remote `--source-config` mode changes; local `--input` behavior stays unchanged.
- Option 2 semantics: if a remote file maps to multiple destination tables, download a separate remote copy into each destination table directory.
- Preserve current parser behavior: each remote file is parsed once in this phase.
- Do not implement parallel parsing or grouped parser return types in this phase.
- Do not change CLI arguments.

---

### Task 1: Route Expansion Helpers

**Files:**
- Modify: `crates/remote-file-source/src/remote.rs`

**Interfaces:**
- Produces: `DownloadTarget { remote_index, route_index, remote_file, local_path }`
- Produces: helper `download_targets(config, remote_files, router)`

- [ ] **Step 1: Write failing tests**

Add tests in `crates/remote-file-source/src/remote.rs`:

```rust
    #[test]
    fn download_targets_expand_dest_table_routes() {
        let config = test_config_with_download_dir(PathBuf::from("downloads"));
        let remote_files = vec!["/remote/NRCELLDU.csv.gz".to_string()];
        let targets = download_targets(&config, &remote_files, &|_| {
            vec!["TPD_A".to_string(), "TPD_B".to_string()]
        });

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].local_path, PathBuf::from("downloads/tpd_a/NRCELLDU.csv.gz"));
        assert_eq!(targets[1].local_path, PathBuf::from("downloads/tpd_b/NRCELLDU.csv.gz"));
    }

    #[test]
    fn download_targets_fallback_to_legacy_location_without_routes() {
        let config = test_config_with_download_dir(PathBuf::from("downloads"));
        let remote_files = vec!["/remote/NRCELLDU.csv.gz".to_string()];
        let targets = download_targets(&config, &remote_files, &|_| Vec::new());

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].local_path, PathBuf::from("downloads/NRCELLDU.csv.gz"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p remote-file-source download_targets_`

Expected: FAIL because helper functions do not exist.

- [ ] **Step 3: Implement helpers**

Add private `DownloadTarget` and helper functions in `remote.rs`. Route names must be lowercased and sanitized with ASCII alphanumeric, `_`, `-`, `.` preserved; all other characters become `_`.

- [ ] **Step 4: Verify focused tests**

Run: `cargo test -p remote-file-source download_targets_`

Expected: PASS.

---

### Task 2: Routed Remote Downloads

**Files:**
- Modify: `crates/remote-file-source/src/lib.rs`
- Modify: `crates/remote-file-source/src/remote.rs`

**Interfaces:**
- Produces: `resolve_files_with_router<F>(options: ResolveOptions, router: F) -> Result<Vec<PathBuf>> where F: Fn(&str) -> Vec<String>`
- Keeps: `resolve_files(options)` as legacy wrapper using an empty router.

- [ ] **Step 1: Extend remote API**

Add `resolve_files_with_router` in `lib.rs` and route matched remote files through `remote::download_files_with_router`.

- [ ] **Step 2: Preserve legacy API**

Change `resolve_files(options)` to call `resolve_files_with_router(options, |_| Vec::new())`.

- [ ] **Step 3: Update sequential downloads**

Sequential mode iterates expanded `DownloadTarget`s and downloads every target. Return only the first local path for each `remote_index`.

- [ ] **Step 4: Update parallel downloads**

Parallel mode queues expanded `DownloadTarget`s. Sort successes by `(remote_index, route_index)` and return only route `0` for each remote file.

- [ ] **Step 5: Verify tests**

Run: `cargo test -p remote-file-source`

Expected: all remote-file-source tests pass.

---

### Task 3: Root Crate Router From Rules

**Files:**
- Modify: `src/main.rs`
- Modify: `src/tpd.rs` if a small public helper is needed

**Interfaces:**
- Produces: helper in `main.rs` that maps `source_table -> Vec<dest_table>` from enabled TPD rule groups.
- Consumes: `remote_file_source::resolve_files_with_router`.

- [ ] **Step 1: Move rule loading before `resolve_files`**

`src/main.rs` already parses mapping before resolving inputs. Move rule-file discovery/loading before `resolve_files` so the router can use rules.

- [ ] **Step 2: Build router closure**

For each remote file path:
- Use `config::detect_counter_from_filename` and `config::resolve_table` against `ctx.mapping` to infer source table.
- Look up destination tables that consume that source table.
- Return destination table names as lowercase route labels.
- Return an empty vector if no route is known, preserving legacy download location.

- [ ] **Step 3: Call routed resolver**

Use `remote_file_source::resolve_files_with_router` instead of `resolve_files`.

- [ ] **Step 4: Verify full tests**

Run: `cargo test`

Expected: all tests pass.

---

### Task 4: Final Verification and Commit

**Files:**
- Modify: `crates/remote-file-source/src/lib.rs`
- Modify: `crates/remote-file-source/src/remote.rs`
- Modify: `src/main.rs`
- Create: `docs/superpowers/plans/2026-06-23-dest-table-download-isolation.md`

- [ ] **Step 1: Format and verify**

Run: `cargo fmt && cargo test && cargo build --release --locked`

Expected: tests and build pass.

- [ ] **Step 2: Optional smoke command**

Run one known remote window and confirm files appear under destination-table subdirectories in `downloads/`.

- [ ] **Step 3: Commit locally**

Run:

```bash
git add crates/remote-file-source/src/lib.rs crates/remote-file-source/src/remote.rs src/main.rs docs/superpowers/plans/2026-06-23-dest-table-download-isolation.md
git commit -m "feat: isolate downloads by destination table"
```

- [ ] **Step 4: Do not push unless requested**

Report the local commit hash and verification results.
