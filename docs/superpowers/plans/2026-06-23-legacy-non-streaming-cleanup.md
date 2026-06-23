# Legacy Non-Streaming Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Commit the parser file-ending normalization separately and remove legacy non-streaming TPD execution/writer code.

**Architecture:** The runtime pipeline is already streaming-only. This cleanup removes unused non-streaming row aggregation and package-writing paths while keeping streaming expression evaluation and streaming output writer behavior unchanged.

**Tech Stack:** Rust 2021, existing unit tests, `cargo test --workspace`, `cargo build --release --locked`.

## Global Constraints

- Do not change streaming runtime behavior.
- Do not reintroduce non-streaming TPD fallback.
- Commit `src/parser.rs` final newline normalization separately.
- Remove only code that is no longer used by the streaming main flow or retained streaming tests.
- Verify with `cargo test --workspace` and `cargo build --release --locked`.

---

### Task 1: Parser File Ending Commit

**Files:**
- Modify: `src/parser.rs`

**Interfaces:**
- Produces: a standalone formatting commit with no behavior change.

- [ ] **Step 1: Inspect parser diff**

Run: `git diff -- src/parser.rs`

Expected: only final newline normalization.

- [ ] **Step 2: Commit parser formatting**

Run:

```bash
git add src/parser.rs
git commit -m "chore: normalize parser file ending"
```

---

### Task 2: Remove Legacy Non-Streaming TPD Path

**Files:**
- Modify: `src/tpd.rs`

**Interfaces:**
- Removes: `pub fn execute_tpd_rule(rule: &TpdRule, tables: &mut TableRows) -> Result<()>`
- Removes: tests that only exercise the deleted non-streaming row aggregation path.
- Keeps: expression helpers and tests used by streaming compiled expressions.

- [ ] **Step 1: Delete `execute_tpd_rule`**

Remove the function body and any helper code that becomes unused only because of that deletion.

- [ ] **Step 2: Delete non-streaming-only test**

Remove the `combines_available_source_tables_and_skips_missing_sources` test because it validates `execute_tpd_rule` behavior that no longer exists.

- [ ] **Step 3: Run focused tpd tests**

Run: `cargo test tpd::tests::`

Expected: remaining TPD tests pass.

---

### Task 3: Remove Legacy Row Writer Path

**Files:**
- Modify: `src/writer.rs`

**Interfaces:**
- Removes: `write_tables`.
- Removes: `write_package`, `write_csv`, `group_rows_by_scan_start`, and `infer_headers` if unused after `write_tables` removal.
- Keeps: `StreamingTableWriter` and helpers used by streaming output.

- [ ] **Step 1: Delete `write_tables` path**

Remove `write_tables` and direct helper functions used only by that path.

- [ ] **Step 2: Remove now-unused imports**

Remove unused imports such as `HashSet` or `Row` if the compiler reports them unused.

- [ ] **Step 3: Run writer tests**

Run: `cargo test writer::tests::`

Expected: streaming writer tests pass.

---

### Task 4: Final Verification and Commit

**Files:**
- Modify: `src/tpd.rs`
- Modify: `src/writer.rs`
- Create: `docs/superpowers/plans/2026-06-23-legacy-non-streaming-cleanup.md`

- [ ] **Step 1: Format and verify**

Run:

```bash
cargo fmt
cargo test --workspace
cargo build --release --locked
```

Expected: tests and release build pass without warnings.

- [ ] **Step 2: Inspect status and diff**

Run: `git status --short --branch` and inspect the intended diff. No unrelated files should be staged.

- [ ] **Step 3: Commit cleanup**

Run:

```bash
git add src/tpd.rs src/writer.rs docs/superpowers/plans/2026-06-23-legacy-non-streaming-cleanup.md
git commit -m "refactor: remove legacy non-streaming paths"
```
