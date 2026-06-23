# Streaming Output Optimization Design

## Goal

Optimize only the streaming TPD hot path by reducing output-stage `Row` lookups and duplicate temp expression evaluation. Do not change non-streaming `execute_tpd_rule`, rule JSON shape, parser behavior, or CLI arguments.

## Scope

- In scope: `StreamingTpdEngine`, `StreamingRuleAggregator`, and `StreamingTableWriter` usage from streaming TPD finish.
- In scope: direct ordered-value writes for streaming output CSV rows.
- In scope: shared temp expression cache within a single merged `StreamingRuleAggregator` and a single group.
- Out of scope: non-streaming aggregation in `execute_tpd_rule`.
- Out of scope: DataFrame/Polars aggregation, rule format changes, and download/cache behavior.

## Current Problem

`StreamingRuleAggregator::finish_plan` currently builds a full output `Row`, then `StreamingTableWriter::write_row` loops through headers and looks every value up by string key. Wide rules such as `TPD_NRCELLDU_PRB_Q_5G` do this for hundreds of fields across tens of thousands of groups.

Merged streaming aggregators also run each plan independently. If two rules share the same source/group and define identical temp fields, each plan recomputes those temp expressions for every group.

## Design

### Direct Streaming Writer Values

Add a streaming-specific writer method:

```rust
pub fn write_values(&mut self, scan_start_time: &str, values: &[String]) -> Result<()>;
```

The method will:

- Use `scan_start_time` to create or select the target package.
- Write `values` directly with `csv::Writer::write_record`.
- Update package and total row counters.

`write_row(&Row)` remains available for existing callers and non-streaming compatibility.

### Output Evaluation Flow

`finish_plan` will still evaluate output fields in rule order to preserve existing expression semantics. It will maintain:

- `output: Row` for expressions that refer to earlier output fields.
- `output_values: Vec<String>` in `headers` order for direct CSV writing.

After all output fields are evaluated, `fill_output_time_from_context` runs as before. Then `output_values` is populated from the final `output` using `headers` order exactly once, and `write_values(scan_start_time, &output_values)` writes the row.

This avoids the writer doing another per-header string lookup and keeps duplicate-output-header behavior aligned with `unique_output_headers`.

### Shared Temp Cache

For each `StreamingGroupState`, create a group-local cache while finishing all merged plans:

```text
cache key = normalized temp field name + unit separator + expression
cache value = evaluated String
```

Each plan still gets its own `context` row. When processing temp fields:

- If the cache contains the exact temp name/expression pair, insert the cached value into the plan context.
- Otherwise evaluate the temp expression, store it in the cache, and insert it into the plan context.

Sharing requires both temp field name and expression to match. Expressions with the same text but different temp names are not shared because downstream expressions may depend on the name.

The cache lives only for one group and one aggregator finish pass, preventing cross-group contamination.

## Error Handling

- Missing `scan_start_time` remains an error before writing.
- Expression errors keep the existing contextual messages identifying rule, field, and expression.
- `write_values` validates that the provided row length matches `headers.len()` and errors if it does not.

## Tests

Add focused unit tests for:

- `StreamingTableWriter::write_values` writes a package with the expected CSV row order.
- Streaming finish output still matches current `Row` semantics for `scan_start_time` filling.
- Identical temp name/expression pairs are evaluated once per group and reused across merged plans where possible.

Run:

```bash
cargo test
cargo build --release --locked
```

Manual regression checks should reuse known windows:

- `2026-06-15 13:45:00` for `TPD_EUTRANCELL_0_Q`.
- `2026-06-17 14:45:00` for `TPD_EUTR_PRB_Q`.
- `2026-06-17 15:15:00` for NR DU/CU rules.

Compare representative output rows against `valid1`, excluding `insert_time` / `collect_time`.

## Success Criteria

- All Rust tests pass.
- Locked release build passes.
- Representative output rows match `valid1` excluding current-time fields.
- Streaming finish timings for large output tables do not regress; expected improvement is most visible on wide rules such as `TPD_NRCELLDU_PRB_Q_5G` and `Tpd_NRCELLDU_q_5g`.
