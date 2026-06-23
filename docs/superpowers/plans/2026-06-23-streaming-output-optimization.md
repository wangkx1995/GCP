# Streaming Output Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Optimize only the streaming TPD finish path by writing ordered CSV values directly and sharing identical temp expressions across merged streaming plans.

**Architecture:** Add a `StreamingTableWriter::write_values` API while keeping `write_row` unchanged. Refactor `StreamingRuleAggregator::finish` so each group has a temporary cache shared across its merged plans, while each plan still maintains its own `context` and output `Row` for expression compatibility.

**Tech Stack:** Rust 2021, `anyhow`, `csv`, `indexmap`, existing streaming TPD engine.

## Global Constraints

- Only change the streaming TPD hot path: `StreamingTpdEngine`, `StreamingRuleAggregator`, and `StreamingTableWriter` usage from streaming finish.
- Do not change non-streaming `execute_tpd_rule`.
- Do not change rule JSON shape, parser behavior, CLI arguments, or download/cache behavior.
- Preserve output CSV headers, output value order, and current-time field behavior.
- Keep `write_row(&Row)` available for existing callers.

---

### Task 1: Direct Streaming Writer Values

**Files:**
- Modify: `src/writer.rs`

**Interfaces:**
- Produces: `StreamingTableWriter::write_values(&mut self, scan_start_time: &str, values: &[String]) -> Result<()>`
- Consumes later: `src/tpd.rs` will pass header-ordered output values to this method.

- [ ] **Step 1: Add a failing writer unit test**

Add this test module near the end of `src/writer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn load_config() -> LoadConfig {
        LoadConfig {
            clickhouse: crate::load_config::ClickHouseConfig {
                client: "clickhouse-client".to_string(),
                host: "127.0.0.1".to_string(),
                port: 9000,
                user: "default".to_string(),
                password: String::new(),
                database: "default".to_string(),
                table_name_case: "lower".to_string(),
            },
            postgresql: crate::load_config::PostgresConfig {
                client: "psql".to_string(),
                host: "127.0.0.1".to_string(),
                port: 5432,
                user: "postgres".to_string(),
                password: String::new(),
                database: "postgres".to_string(),
            },
        }
    }

    #[test]
    fn streaming_writer_writes_ordered_values_directly() {
        let dir = tempdir().unwrap();
        let load_config = load_config();
        let headers = vec!["scan_start_time".to_string(), "name".to_string()];
        let mut writer = StreamingTableWriter::new_with_headers(
            headers,
            "TPD_TEST",
            dir.path(),
            b'|',
            "collect_1",
            LoadType::Clickhouse,
            &load_config,
        )
        .unwrap();

        writer
            .write_values(
                "2026-06-17 15:15:00",
                &["2026-06-17 15:15:00".to_string(), "cell-1".to_string()],
            )
            .unwrap();
        writer.finish().unwrap();

        let csv_path = dir
            .path()
            .join("tpd_test_2026061715")
            .join("collect_1_202606171515")
            .join("tpd_test.csv");
        let text = std::fs::read_to_string(csv_path).unwrap();
        assert_eq!(text.trim_end(), "2026-06-17 15:15:00|cell-1");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test streaming_writer_writes_ordered_values_directly`

Expected: FAIL with no method named `write_values`.

- [ ] **Step 3: Implement `write_values`**

Add to `impl StreamingTableWriter<'_>` in `src/writer.rs`:

```rust
    pub fn write_values(&mut self, scan_start_time: &str, values: &[String]) -> Result<()> {
        if values.len() != self.headers.len() {
            anyhow::bail!(
                "streaming output value count mismatch for {}: got {}, expected {}",
                self.table,
                values.len(),
                self.headers.len()
            );
        }
        if !self.packages.contains_key(scan_start_time) {
            let package = create_streaming_package(
                &self.options,
                &self.table,
                &self.headers,
                parse_scan_start(scan_start_time)?,
            )?;
            self.packages.insert(scan_start_time.to_string(), package);
        }
        let package = self.packages.get_mut(scan_start_time).expect("package exists");
        package.writer.write_record(values)?;
        package.row_count += 1;
        self.total_rows += 1;
        Ok(())
    }
```

- [ ] **Step 4: Run focused and full tests**

Run: `cargo test streaming_writer_writes_ordered_values_directly`

Expected: PASS.

Run: `cargo test`

Expected: all tests pass.

---

### Task 2: Use Ordered Values From Streaming Finish

**Files:**
- Modify: `src/tpd.rs`

**Interfaces:**
- Consumes: `StreamingTableWriter::write_values(&mut self, scan_start_time: &str, values: &[String]) -> Result<()>`
- Produces: helper `ordered_output_values(headers: &[String], output: &Row) -> Vec<String>` inside `src/tpd.rs`.

- [ ] **Step 1: Add a focused helper test**

Add this test inside the existing `#[cfg(test)] mod tests` in `src/tpd.rs`:

```rust
    #[test]
    fn ordered_output_values_follow_unique_headers() {
        let headers = vec!["a".to_string(), "b".to_string()];
        let mut output = Row::new();
        output.insert("b".to_string(), "2".to_string());
        output.insert("a".to_string(), "1".to_string());

        assert_eq!(ordered_output_values(&headers, &output), vec!["1", "2"]);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test ordered_output_values_follow_unique_headers`

Expected: FAIL with missing `ordered_output_values`.

- [ ] **Step 3: Add helper and switch `finish_plan` writer call**

Add near `unique_output_headers` in `src/tpd.rs`:

```rust
fn ordered_output_values(headers: &[String], output: &Row) -> Vec<String> {
    headers
        .iter()
        .map(|header| output.get(header).cloned().unwrap_or_default())
        .collect()
}
```

Change the end of `finish_plan` per row from:

```rust
fill_output_time_from_context(&mut output, &context, row);
writer.write_row(&output)?;
```

to:

```rust
fill_output_time_from_context(&mut output, &context, row);
let scan_start_time = output
    .get("scan_start_time")
    .context("output row missing scan_start_time")?;
let output_values = ordered_output_values(&headers, &output);
writer.write_values(scan_start_time, &output_values)?;
```

- [ ] **Step 4: Run tests**

Run: `cargo test ordered_output_values_follow_unique_headers`

Expected: PASS.

Run: `cargo test`

Expected: all tests pass.

---

### Task 3: Shared Temp Cache Across Merged Streaming Plans

**Files:**
- Modify: `src/tpd.rs`

**Interfaces:**
- Produces: `TempCache = FastHashMap<String, String>` local to `finish` / group iteration.
- Produces: helper `temp_cache_key(field: &CompiledFieldExpr<'_>) -> String`.
- Produces: helper method `finish_plan_row(...)` if needed to keep `finish` readable.

- [ ] **Step 1: Add a test for temp key stability**

Add this test inside `src/tpd.rs` tests:

```rust
    #[test]
    fn temp_cache_key_requires_name_and_expression() {
        let left = FieldRule {
            name: "vendor_id_0".to_string(),
            expression: "lower(max(VENDORNAME))".to_string(),
            related_group: "related_rdn01".to_string(),
        };
        let same = FieldRule {
            name: "vendor_id_0".to_string(),
            expression: "lower(max(VENDORNAME))".to_string(),
            related_group: "related_rdn02".to_string(),
        };
        let different_name = FieldRule {
            name: "vendor_id_1".to_string(),
            expression: "lower(max(VENDORNAME))".to_string(),
            related_group: "related_rdn01".to_string(),
        };
        let field_indexes = FastHashMap::default();
        let left = CompiledFieldExpr::new(&left, &field_indexes);
        let same = CompiledFieldExpr::new(&same, &field_indexes);
        let different_name = CompiledFieldExpr::new(&different_name, &field_indexes);

        assert_eq!(temp_cache_key(&left), temp_cache_key(&same));
        assert_ne!(temp_cache_key(&left), temp_cache_key(&different_name));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test temp_cache_key_requires_name_and_expression`

Expected: FAIL with missing `temp_cache_key`.

- [ ] **Step 3: Implement temp cache key**

Add near `CompiledFieldExpr` helpers:

```rust
fn temp_cache_key(field: &CompiledFieldExpr<'_>) -> String {
    format!(
        "{}\u{1f}{}",
        normalize_lookup_name(&field.field.name),
        field.field.expression.trim()
    )
}
```

- [ ] **Step 4: Refactor finish to share per-group temp cache**

Change `StreamingRuleAggregator::finish` from iterating plans outside groups to iterating groups first, with a group-local cache:

```rust
let mut writers = self.create_plan_writers(options)?;
let mut output_counts = vec![0_usize; self.plans.len()];
for state in self.grouped.values() {
    let mut temp_cache = FastHashMap::default();
    for (plan_idx, plan) in self.plans.iter().enumerate() {
        self.finish_plan_row(plan, state, &mut temp_cache, &mut writers[plan_idx])?;
        output_counts[plan_idx] += 1;
    }
}
```

Keep existing log lines per plan. It is acceptable to preserve `finish_plan` shape by passing an optional `temp_cache` if that is smaller.

When evaluating temp fields, use:

```rust
let cache_key = temp_cache_key(field);
let value = if let Some(value) = temp_cache.get(&cache_key) {
    value.clone()
} else {
    let value = field.eval(state, &rows, &context, None).with_context(...)?;
    temp_cache.insert(cache_key, value.clone());
    value
};
context.insert(field.field.name.trim().to_string(), value);
```

- [ ] **Step 5: Run tests and build**

Run: `cargo test`

Expected: all tests pass.

Run: `cargo build --release --locked`

Expected: build succeeds.

---

### Task 4: Regression Commands

**Files:**
- No code changes required.

- [ ] **Step 1: Run representative collection for NR**

Run:

```bash
cargo run --release -- \
  --source-config source.toml \
  --scan-start-time "2026-06-17 15:15:00" \
  --config-dir . \
  --output-dir valid/remote \
  --collect-id tpd_2026051716564812850 \
  --load-type clickhouse \
  --load-config load.toml \
  --encoding UTF-8 \
  --rules-dir rules
```

Expected: `TPD_NRCELLDU_PRB_Q_5G`, `Tpd_NRCELLDU_q_5g`, and `Tpd_NRCELLCU_q_5g` stream successfully.

- [ ] **Step 2: Run representative collection for EUTR PRB**

Run the same command with `--scan-start-time "2026-06-17 14:45:00"`.

Expected: `TPD_EUTR_PRB_Q` streams successfully.

- [ ] **Step 3: Run representative collection for EUTRANCELL**

Run the same command with `--scan-start-time "2026-06-15 13:45:00"`.

Expected: `TPD_EUTRANCELL_0_Q` streams successfully.

- [ ] **Step 4: Compare representative rows**

Compare each output against `valid1`, excluding `insert_time` and `collect_time`. Expected: first-row mismatch count is zero for each checked table.

---

### Task 5: Final Commit

**Files:**
- Modify: `src/writer.rs`
- Modify: `src/tpd.rs`
- Create: `docs/superpowers/plans/2026-06-23-streaming-output-optimization.md`

- [ ] **Step 1: Inspect status and diff**

Run: `git status --short --branch`

Run: `git diff --stat`

Expected: only intended files are changed.

- [ ] **Step 2: Commit implementation**

Run:

```bash
git add src/writer.rs src/tpd.rs docs/superpowers/plans/2026-06-23-streaming-output-optimization.md
git commit -m "perf: optimize streaming output finish"
```

- [ ] **Step 3: Do not push unless requested**

Report the local commit hash and verification results.
