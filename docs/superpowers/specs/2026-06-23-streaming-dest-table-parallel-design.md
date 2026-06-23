# Streaming Dest Table Parallel Design

## Goal

Make TPD processing streaming-only and prepare parsing/output to run independently per destination table.

## Decisions

- Remove the non-streaming TPD execution path from the main pipeline.
- Treat every loaded rule as required to be streaming-compatible.
- Fail at startup when any rule cannot be represented by the streaming engine.
- Parallelize only by destination table in this phase.
- Keep each destination table single-threaded internally to avoid writer and aggregator sharing.
- Add `--streaming-parallel <N>` with default `1` so current behavior stays conservative unless explicitly increased.

## Streaming Compatibility

A rule is streaming-compatible when `StreamingRuleAggregator::new(rule)` can build an aggregator. With current engine constraints, this means the rule has exactly one enabled group and uses expressions supported by the streaming evaluator.

Startup validation reports all incompatible rules before failing. The error includes the rule table name and the reason, for example:

```text
rule TPD_X is not streaming-compatible: expected exactly one enabled group
```

There is no non-streaming fallback after this change.

## Input Grouping

Remote mode uses the destination-table download isolation already introduced:

```text
downloads/<dest_table_lower>/<remote_file_name>
```

`remote-file-source` will expose grouped routed inputs:

```rust
pub struct RoutedInputs {
    pub representative_files: Vec<PathBuf>,
    pub groups: Vec<RoutedInputGroup>,
}

pub struct RoutedInputGroup {
    pub route: String,
    pub files: Vec<PathBuf>,
}
```

The existing `resolve_files` API remains for compatibility. The streaming-only main flow uses grouped inputs when rules are present.

Local `--input` mode has no pre-isolated download directories. For this phase, each streaming destination table receives the same local input list and filters rows through its own rule source tables during parsing. This preserves local testability and avoids adding local file routing heuristics.

## Per Table Execution

The main pipeline builds a work item per destination table:

```rust
struct StreamingTableTask {
    dest_table: String,
    rules: Vec<TpdRule>,
    inputs: Vec<PathBuf>,
}
```

Each task:

- Creates a `StreamingTpdEngine` from only that table's rules.
- Computes required and ordered fields from only that table's rules.
- Parses only that table's input files.
- Accepts only rows for source tables consumed by that table's engine.
- Calls `finish` with an empty `TableRows`, causing streaming output packages to be written directly.

No task shares a writer, aggregator, or mutable table map with another task.

## Parallelism

`--streaming-parallel` controls the maximum number of destination-table tasks running at once.

- `1`: run destination tables sequentially.
- `N > 1`: run up to `N` destination tables concurrently.

The implementation can use scoped threads so task closures can borrow loaded rules safely without cloning large rule structures. Result ordering in logs is not guaranteed when parallelism is enabled, but output paths remain deterministic because each task writes a distinct destination table.

## Removed Main-Flow Behavior

The following main-flow concepts are removed:

- `non_streaming_source_tables`.
- Retaining parsed source rows solely for non-streaming TPD rules.
- Calling `tpd::execute_tpd_rule` after parsing.
- Writing non-streaming TPD output via `writer::write_tables` for TPD rules.

`execute_tpd_rule` can remain in `tpd.rs` temporarily for existing unit tests or future cleanup, but `main.rs` no longer calls it.

## Error Handling

- Unsupported rules fail before remote downloads when possible.
- Missing routed input groups fail if the destination table has no local or remote files.
- Any task failure fails the whole command.
- Parallel task errors are collected and reported with the destination table name.

## Testing

- Unit-test streaming compatibility validation with compatible and incompatible rules.
- Unit-test routed input grouping returns one group per destination table route.
- Unit-test task construction maps each destination table to only its own rules.
- Keep existing streaming aggregation tests.
- Verify with:

```bash
cargo test --workspace
cargo build --release --locked
```

## Success Criteria

- All configured TPD rules run through streaming only.
- Incompatible rules fail fast with actionable messages.
- Remote NRCELLDU files downloaded under multiple destination-table directories are parsed separately by each destination table task.
- `--streaming-parallel 1` preserves deterministic sequential execution.
- `--streaming-parallel N` can process multiple destination tables concurrently without shared mutable aggregation state.
