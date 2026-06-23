use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use indexmap::IndexMap;
use remote_file_source::ResolveOptions;
use tempfile::TempDir;

mod config;
mod crc64;
mod load_config;
mod parser;
mod tpd;
mod util;
mod writer;
use crate::config::ContextData;

type Row = IndexMap<String, String>;
type TableRows = HashMap<String, Vec<Row>>;

#[derive(Parser)]
#[command(name = "wy-gnb-pm-parser")]
#[command(about = "Parse WY GNB PM files into per-table UTF-8 CSV files")]
struct Cli {
    #[arg(long)]
    input: Option<PathBuf>,
    #[arg(long)]
    source_config: Option<PathBuf>,
    #[arg(long)]
    scan_start_time: Option<String>,
    #[arg(long, default_value = ".")]
    config_dir: PathBuf,
    #[arg(long)]
    output_dir: PathBuf,
    #[arg(long)]
    collect_id: String,
    #[arg(long, value_enum)]
    load_type: LoadType,
    #[arg(long, default_value = "load.toml")]
    load_config: PathBuf,
    #[arg(long, default_value = "|")]
    output_delimiter: String,
    #[arg(long, default_value = "UTF-8")]
    encoding: String,
    #[arg(long)]
    recursive: bool,
    #[arg(long = "rule-file")]
    rule_files: Vec<PathBuf>,
    #[arg(long = "rules-dir")]
    rules_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum LoadType {
    Postgresql,
    Clickhouse,
}

fn main() -> Result<()> {
    let start = Instant::now();
    let cli = Cli::parse();
    let mapping_path = cli.config_dir.join("mapping_dx.ini");
    let output_delimiter = parse_delimiter(&cli.output_delimiter)?;
    let load_config = load_config::load_config(&cli.load_config)
        .with_context(|| format!("failed to parse {}", cli.load_config.display()))?;
    let mapping = config::parse_mapping_config(&mapping_path)
        .with_context(|| format!("failed to parse {}", mapping_path.display()))?;
    let ctx = ContextData {
        mapping,
        encoding: cli.encoding,
    };

    let rule_files = discover_rule_files(cli.rule_files, cli.rules_dir.as_ref())?;
    let mut rules = Vec::new();
    for rule_file in &rule_files {
        eprintln!("[rule] loading {}", rule_file.display());
        rules.push(tpd::load_rule(rule_file)?);
    }
    let dest_tables_by_source = dest_tables_by_source_table(&rules);

    let temp_dir = TempDir::new().context("failed to create temp dir")?;
    let mut tables = TableRows::new();
    let inputs = remote_file_source::resolve_files_with_router(
        ResolveOptions {
            local_input: cli.input,
            recursive: cli.recursive,
            source_config: cli.source_config,
            scan_start_time: cli.scan_start_time,
        },
        |remote_file| route_remote_file(remote_file, &ctx, &dest_tables_by_source),
    )?;

    let streaming_source_tables = tpd::streaming_source_tables(&rules);
    let streaming_rule_tables = tpd::streaming_rule_tables(&rules);
    let streaming_required_fields = tpd::streaming_required_fields_by_table(&rules);
    let streaming_ordered_fields = tpd::streaming_ordered_fields_by_table(&rules);
    let non_streaming_source_tables =
        non_streaming_source_tables(&rules, &streaming_rule_tables, &streaming_source_tables);
    let streaming_engine = std::cell::RefCell::new(tpd::StreamingTpdEngine::new(&rules));
    if !streaming_engine.borrow().is_empty() {
        eprintln!(
            "[aggregate] streaming source tables: {:?}",
            streaming_source_tables
        );
    }

    eprintln!("[input] {} file(s) to process", inputs.len());
    for input in &inputs {
        parser::parse_path_with_streaming_values(
            &ctx,
            input,
            temp_dir.path(),
            &streaming_required_fields,
            &streaming_ordered_fields,
            &mut |table, row| {
                let table_upper = table.to_ascii_uppercase();
                let stream_consumed = streaming_engine.borrow().consumes_table(&table_upper);
                let keep_rows =
                    !stream_consumed || non_streaming_source_tables.contains(&table_upper);

                if stream_consumed && !keep_rows {
                    streaming_engine
                        .borrow_mut()
                        .accept_owned(&table_upper, row)?;
                } else {
                    if stream_consumed {
                        streaming_engine.borrow_mut().accept(&table_upper, &row)?;
                    }
                    tables.entry(table_upper).or_default().push(row);
                }
                Ok(())
            },
            &mut |table, values| {
                streaming_engine
                    .borrow_mut()
                    .accept_values(&table.to_ascii_uppercase(), values)
            },
        )
        .with_context(|| format!("failed to parse {}", input.display()))?;
    }
    let streaming_finish_options = tpd::StreamingFinishOptions {
        output_dir: &cli.output_dir,
        delimiter: output_delimiter,
        collect_id: &cli.collect_id,
        load_type: cli.load_type,
        load_config: &load_config,
    };
    streaming_engine
        .into_inner()
        .finish(&mut tables, &streaming_finish_options)?;

    for (rule_file, rule) in rule_files.iter().zip(&rules) {
        if streaming_rule_tables.contains(&rule.table_name.to_ascii_uppercase()) {
            continue;
        }
        tpd::execute_tpd_rule(rule, &mut tables)
            .with_context(|| format!("failed to execute rule {}", rule_file.display()))?;
    }

    writer::write_tables(
        &ctx.mapping,
        &tables,
        &cli.output_dir,
        output_delimiter,
        &cli.collect_id,
        cli.load_type,
        &load_config,
    )?;
    eprintln!("[done] {:.2}s total", start.elapsed().as_secs_f64());
    Ok(())
}

fn discover_rule_files(
    mut rule_files: Vec<PathBuf>,
    rules_dir: Option<&PathBuf>,
) -> Result<Vec<PathBuf>> {
    if let Some(rules_dir) = rules_dir {
        let mut entries: Vec<_> = fs::read_dir(rules_dir)
            .with_context(|| format!("failed to read rules dir {}", rules_dir.display()))?
            .filter_map(|entry| entry.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| p.extension().map(|ext| ext == "json").unwrap_or(false))
            .collect();
        entries.sort();
        if entries.is_empty() {
            eprintln!("[rule] no .json files found in {}", rules_dir.display());
        }
        rule_files.extend(entries);
    }
    Ok(rule_files)
}

fn dest_tables_by_source_table(rules: &[tpd::TpdRule]) -> HashMap<String, Vec<String>> {
    let mut dest_tables_by_source: HashMap<String, Vec<String>> = HashMap::new();
    for rule in rules {
        let dest_table = rule.table_name.to_ascii_uppercase();
        for group in rule.groups.iter().filter(|group| group.enabled) {
            for source_table in &group.source_table {
                let tables = dest_tables_by_source
                    .entry(source_table.to_ascii_uppercase())
                    .or_default();
                if !tables.contains(&dest_table) {
                    tables.push(dest_table.clone());
                }
            }
        }
    }
    dest_tables_by_source
}

fn route_remote_file(
    remote_file: &str,
    ctx: &ContextData,
    dest_tables_by_source: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let path = PathBuf::from(remote_file);
    let Ok(counter) = config::detect_counter_from_filename(&path, &ctx.mapping) else {
        return Vec::new();
    };
    let Ok(source_table) = config::resolve_table(&ctx.mapping, &counter, &path) else {
        return Vec::new();
    };
    dest_tables_by_source
        .get(&source_table.to_ascii_uppercase())
        .cloned()
        .unwrap_or_default()
}

fn non_streaming_source_tables(
    rules: &[tpd::TpdRule],
    streaming_rule_tables: &std::collections::HashSet<String>,
    streaming_source_tables: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    rules
        .iter()
        .filter(|rule| !streaming_rule_tables.contains(&rule.table_name.to_ascii_uppercase()))
        .flat_map(|rule| rule.groups.iter())
        .filter(|group| group.enabled)
        .flat_map(|group| group.source_table.iter())
        .map(|table| table.to_ascii_uppercase())
        .filter(|table| streaming_source_tables.contains(table))
        .collect()
}

fn parse_delimiter(value: &str) -> Result<u8> {
    let bytes = value.as_bytes();
    if bytes.len() != 1 {
        bail!("output delimiter must be exactly one ASCII byte, got {value:?}");
    }
    Ok(bytes[0])
}
