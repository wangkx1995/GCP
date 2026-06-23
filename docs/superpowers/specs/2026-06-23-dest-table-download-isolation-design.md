# Dest Table Download Isolation Design

## Goal

When one matched remote file feeds multiple destination tables, download one copy per destination-table directory so later parser work can process each table's directory independently.

## Scope

- In scope: remote `--source-config` downloads only.
- In scope: route matched remote files to destination-table subdirectories under `source.download_dir`.
- In scope: keep current `resolve_files` behavior compatible by returning one representative local path per remote file for now.
- Out of scope: parallel parsing by destination table; that is the next spec.
- Out of scope: executing `load.ctl` or changing output package structure.

## Design

`remote-file-source` stays generic and receives an optional router callback from the root crate. The router maps a remote path to zero or more destination table names.

The root crate builds that router from loaded TPD rules:

- For every enabled group, map each `source_table` to the rule's `table_name`.
- For each matched remote file, detect its source table using `mapping_dx.ini` filename logic.
- Return all destination tables that consume that source table.

For each route, remote download writes to:

```text
<download_dir>/<dest_table_lower>/<remote_file_name>
```

If no route is found, download falls back to the original location:

```text
<download_dir>/<remote_file_name>
```

Because parser grouping is not implemented in this phase, `resolve_files` returns only the first downloaded path per remote file. This preserves current parsing semantics and prevents duplicate aggregation.

## Error Handling

- Any failed destination copy/download fails the whole download batch.
- `download_parallel` still controls worker count.
- Parallel mode creates tasks per `(remote file, destination table)` so option 2 intentionally downloads separate remote copies.
- Returned file order remains ordered by original remote match order.

## Tests

- Unit-test route expansion: one remote file with two destination tables produces two target paths and one representative returned path.
- Unit-test fallback route: no destination table produces the legacy `<download_dir>/<filename>` path.
- Existing config tests continue to cover `download_parallel > 0`.

## Success Criteria

- `cargo test` passes.
- `cargo build --release --locked` passes.
- Existing remote runs still parse each matched remote file once.
- Download directories are ready for later per-destination-table parsing.
