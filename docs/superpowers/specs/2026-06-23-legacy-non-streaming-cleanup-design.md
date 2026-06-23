# Legacy Non-Streaming Cleanup Design

## Goal

Remove legacy non-streaming TPD execution code now that the parser pipeline is streaming-only by destination table.

## Scope

- Commit the existing `src/parser.rs` final newline normalization separately.
- Remove the old `tpd::execute_tpd_rule` path from `src/tpd.rs`.
- Remove the old non-streaming `writer::write_tables` package writer path from `src/writer.rs`.
- Keep streaming aggregation, streaming writer, and expression helpers that streaming tests still use.
- Do not change runtime behavior for streaming rules.

## Cleanup Details

`src/tpd.rs` cleanup:

- Delete `execute_tpd_rule`.
- Delete tests that only validate the removed non-streaming multi-source row aggregation path.
- Keep expression evaluator tests used by compiled streaming expressions.

`src/writer.rs` cleanup:

- Delete `write_tables`.
- Delete helper functions used only by `write_tables`: `write_package`, `write_csv`, `group_rows_by_scan_start`, and `infer_headers`.
- Keep `StreamingTableWriter` and package helpers it uses.

## Verification

Run:

```bash
cargo test --workspace
cargo build --release --locked
```

Expected result: tests and release build pass without warnings.
