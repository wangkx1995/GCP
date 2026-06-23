# AGENTS.md - wy-gnb-pm-parser

## What This Is

Rust CLI that parses Woyang 4G/5G PM files (`.csv`, `.gz`, `.zip`) into per-table UTF-8 CSV packages and can aggregate TPD rule JSONs while parsing.

## Commands

```bash
cargo test                         # full test suite; currently unit tests in crc64/tpd/remote-file-source
cargo test <test_name>             # focused Rust test, e.g. cargo test streaming_value_path
cargo build --release              # local release build
cargo build --release --locked     # lockfile-respecting build; useful before pushing
```

CI builds the Linux artifact with musl, not the default GNU target:

```bash
cargo build --release --locked --target x86_64-unknown-linux-musl
```

No repo lint, formatter, clippy, or pre-commit config is wired into CI.

## Architecture Notes

- Workspace members: root binary crate `wy-gnb-pm-parser` and `crates/remote-file-source` for local/SFTP/FTP input resolution.
- Entrypoint is `src/main.rs`; clap args are defined there.
- `src/parser.rs` handles CSV/gz/zip parsing. It has a `fast=values` path used by streaming TPD aggregation.
- `src/tpd.rs` is the rule engine. Current hot path is `StreamingTpdEngine` / `StreamingRuleAggregator`; do not assume Polars exists or reintroduce DataFrame aggregation without an explicit requirement.
- `src/writer.rs` writes package directories containing `<table>.csv`, `<table>.ini`, `load.ctl`, and `result.csv` grouped by `scan_start_time`.
- `crates/remote-file-source` renders `${SCAN_START_TIME...}` templates, scans FTP/SFTP directories, downloads matches into configured `download_dir`, and supports retries/timeouts from `source.toml`.

## Runtime Configs and Inputs

These are runtime inputs, not normal source code fixtures:

- `<config-dir>/mapping_dx.ini`: required table/column mapping. `filenum = -1` means position-based parse; `filenum = 0` means header-based parse.
- `source.toml`: FTP/SFTP source config. Treat as local secret-bearing config.
- `load.toml`: ClickHouse/PostgreSQL load config; supports `${ENV_VAR:-default}` substitution.
- `rules/*.json`: TPD rule files; `rules/` is gitignored but required for `--rules-dir` runs.
- `colNameCutConfig.ini`: column-name normalization overrides.

Representative remote run shape:

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

Use `--input <file-or-dir>` instead of `--source-config`; they are mutually exclusive. `--scan-start-time` is required only for `--source-config`.

## Parsing and Rules Gotchas

- Default output delimiter is `|`; `EASTCOM_PM_OR*` source files are parsed as comma-delimited/header-based.
- `--encoding` defaults to `UTF-8`; non-UTF-8 input falls back out of the streaming values path.
- TPD JSON shape is `table_name`, `groups[]`, `temp_fields[]`, `output_fields[]`.
- Streaming grouping has optimized key patterns for `dn+scan_start_time+scan_stop_time`, `object_rdn+scan_start_time+scan_stop_time`, `dn/RDN+timestamp14(SOURCEFILENAME)`, and simple field lists.
- Supported expression subset includes `max(field)`, `lower(max(field))`, `crc64(...)`, `count(distinct field)`, `substring`, `locate`, `timestamp14`, `case when ... end`, string literals, env literals, and `||` concatenation.
- When validating output against `valid1`, ignore current-time fields such as `insert_time` / `collect_time`.

## Release / Compatibility Notes

- `ssh2` uses `vendored-openssl`; keep this unless target deployment strategy changes.
- CI artifact is `wy-gnb-pm-parser-linux-x86_64-musl` to avoid target-host `libssl.so.3` and `GLIBC_2.xx not found` failures.
- CI runs `ldd` and fails if the artifact dynamically depends on `GLIBC_`, `libc.so`, `libssl`, or `libcrypto`.

## Git / Generated Files

- Gitignored: `/target/`, `/output/`, `/valid/`, `/downloads/`, `/rules/`, `fixtures/*.gz`, `fixtures/*.zip`.
- `valid1/` is not ignored and is used as a checked baseline directory in local comparisons; do not overwrite or delete it casually.
- Keep secret-bearing local config files out of commits.
