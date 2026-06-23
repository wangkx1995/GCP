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
use crate::load_config::LoadConfig;

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
    tpd::validate_streaming_rules(&rules)?;
    let dest_tables_by_source = dest_tables_by_source_table(&rules);

    let routed_inputs = remote_file_source::resolve_routed_files_with_router(
        ResolveOptions {
            local_input: cli.input,
            recursive: cli.recursive,
            source_config: cli.source_config,
            scan_start_time: cli.scan_start_time,
        },
        |remote_file| route_remote_file(remote_file, &ctx, &dest_tables_by_source),
    )?;
    let tasks = build_streaming_table_tasks(
        &rules,
        &routed_inputs.groups,
        &routed_inputs.representative_files,
    )?;
    let streaming_parallel = effective_streaming_parallelism(tasks.len());
    eprintln!(
        "[aggregate] streaming destination tables: {} task(s), parallel={}",
        tasks.len(),
        streaming_parallel
    );
    run_streaming_table_tasks(
        tasks,
        &ctx,
        &cli.output_dir,
        output_delimiter,
        &cli.collect_id,
        cli.load_type,
        &load_config,
    )?;

    eprintln!("[done] {:.2}s total", start.elapsed().as_secs_f64());
    Ok(())
}

fn run_streaming_table_task(
    task: StreamingTableTask,
    ctx: &ContextData,
    output_dir: &std::path::Path,
    output_delimiter: u8,
    collect_id: &str,
    load_type: LoadType,
    load_config: &LoadConfig,
) -> Result<()> {
    let temp_dir = TempDir::new().context("failed to create temp dir")?;
    let streaming_required_fields = tpd::streaming_required_fields_by_table(&task.rules);
    let streaming_ordered_fields = tpd::streaming_ordered_fields_by_table(&task.rules);
    let streaming_engine = std::cell::RefCell::new(tpd::StreamingTpdEngine::new(&task.rules));
    eprintln!(
        "[aggregate] {} input file(s) for {}",
        task.inputs.len(),
        task.dest_table
    );
    for input in &task.inputs {
        parser::parse_path_with_streaming_values(
            ctx,
            input,
            temp_dir.path(),
            &streaming_required_fields,
            &streaming_ordered_fields,
            &mut |table, row| {
                let table_upper = table.to_ascii_uppercase();
                let stream_consumed = streaming_engine.borrow().consumes_table(&table_upper);
                if stream_consumed {
                    streaming_engine
                        .borrow_mut()
                        .accept_owned(&table_upper, row)?;
                }
                Ok(())
            },
            &mut |table, values| {
                let table_upper = table.to_ascii_uppercase();
                if streaming_engine.borrow().consumes_table(&table_upper) {
                    streaming_engine
                        .borrow_mut()
                        .accept_values(&table_upper, values)?;
                }
                Ok(())
            },
        )
        .with_context(|| format!("failed to parse {}", input.display()))?;
    }
    let streaming_finish_options = tpd::StreamingFinishOptions {
        output_dir,
        delimiter: output_delimiter,
        collect_id,
        load_type,
        load_config,
    };
    let mut tables = TableRows::new();
    streaming_engine
        .into_inner()
        .finish(&mut tables, &streaming_finish_options)?;
    Ok(())
}

fn run_streaming_table_tasks(
    tasks: Vec<StreamingTableTask>,
    ctx: &ContextData,
    output_dir: &std::path::Path,
    output_delimiter: u8,
    collect_id: &str,
    load_type: LoadType,
    load_config: &LoadConfig,
) -> Result<()> {
    let parallel = effective_streaming_parallelism(tasks.len());
    let mut pending = tasks.into_iter();
    loop {
        let batch = pending.by_ref().take(parallel).collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }
        let results = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(batch.len());
            for task in batch {
                handles.push(scope.spawn(move || {
                    let dest_table = task.dest_table.clone();
                    let result = run_streaming_table_task(
                        task,
                        ctx,
                        output_dir,
                        output_delimiter,
                        collect_id,
                        load_type,
                        load_config,
                    )
                    .with_context(|| {
                        format!("failed to process streaming destination table {dest_table}")
                    });
                    (dest_table, result)
                }));
            }
            handles
                .into_iter()
                .map(|handle| handle.join())
                .collect::<Vec<_>>()
        });

        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok((_, Ok(()))) => {}
                Ok((dest_table, Err(err))) => errors.push(format!("{dest_table}: {err:#}")),
                Err(_) => errors.push("streaming destination table worker panicked".to_string()),
            }
        }
        if !errors.is_empty() {
            bail!(
                "streaming destination table task(s) failed: {}",
                errors.join("; ")
            );
        }
    }
    Ok(())
}

fn effective_streaming_parallelism(task_count: usize) -> usize {
    task_count
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

struct StreamingTableTask {
    dest_table: String,
    rules: Vec<tpd::TpdRule>,
    inputs: Vec<PathBuf>,
}

fn build_streaming_table_tasks(
    rules: &[tpd::TpdRule],
    routed_groups: &[remote_file_source::RoutedInputGroup],
    fallback_inputs: &[PathBuf],
) -> Result<Vec<StreamingTableTask>> {
    let mut dest_order = Vec::new();
    let mut rules_by_dest: HashMap<String, Vec<tpd::TpdRule>> = HashMap::new();
    for rule in rules {
        let dest_table = rule.table_name.to_ascii_uppercase();
        if !rules_by_dest.contains_key(&dest_table) {
            dest_order.push(dest_table.clone());
        }
        rules_by_dest
            .entry(dest_table)
            .or_default()
            .push(rule.clone());
    }

    let grouped_inputs: HashMap<String, Vec<PathBuf>> = routed_groups
        .iter()
        .map(|group| (group.route.to_ascii_uppercase(), group.files.clone()))
        .collect();
    let use_routed_inputs = !routed_groups.is_empty();
    let mut tasks = Vec::with_capacity(dest_order.len());
    for dest_table in dest_order {
        let inputs = if use_routed_inputs {
            let Some(files) = grouped_inputs
                .get(&dest_table)
                .cloned()
                .filter(|files| !files.is_empty())
            else {
                eprintln!("[input] skip {dest_table}: no routed input files");
                continue;
            };
            files
        } else {
            if fallback_inputs.is_empty() {
                bail!("missing input files for {dest_table}");
            }
            fallback_inputs.to_vec()
        };
        let rules = rules_by_dest
            .remove(&dest_table)
            .expect("destination table must have grouped rules");
        tasks.push(StreamingTableTask {
            dest_table,
            rules,
            inputs,
        });
    }
    Ok(tasks)
}

fn parse_delimiter(value: &str) -> Result<u8> {
    let bytes = value.as_bytes();
    if bytes.len() != 1 {
        bail!("output delimiter must be exactly one ASCII byte, got {value:?}");
    }
    Ok(bytes[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rule(table_name: &str, source_table: &str) -> tpd::TpdRule {
        serde_json::from_str(&format!(
            r#"{{
              "table_name":"{table_name}",
              "groups":[{{"name":"g1","enabled":true,"source_table":"{source_table}","group_by":["dn"]}}],
              "temp_fields":[],
              "output_fields":[{{"name":"dn","expression":"max(dn)"}}]
            }}"#
        ))
        .unwrap()
    }

    #[test]
    fn build_streaming_table_tasks_uses_routed_group_files() {
        let rules = vec![test_rule("TPD_A", "OP_A"), test_rule("TPD_B", "OP_B")];
        let groups = vec![
            remote_file_source::RoutedInputGroup {
                route: "TPD_A".to_string(),
                files: vec![PathBuf::from("downloads/tpd_a/a.csv.gz")],
            },
            remote_file_source::RoutedInputGroup {
                route: "TPD_B".to_string(),
                files: vec![PathBuf::from("downloads/tpd_b/b.csv.gz")],
            },
        ];

        let tasks = build_streaming_table_tasks(&rules, &groups, &[]).unwrap();

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].dest_table, "TPD_A");
        assert_eq!(tasks[0].rules.len(), 1);
        assert_eq!(
            tasks[0].inputs,
            vec![PathBuf::from("downloads/tpd_a/a.csv.gz")]
        );
        assert_eq!(tasks[1].dest_table, "TPD_B");
        assert_eq!(tasks[1].rules.len(), 1);
        assert_eq!(
            tasks[1].inputs,
            vec![PathBuf::from("downloads/tpd_b/b.csv.gz")]
        );
    }

    #[test]
    fn build_streaming_table_tasks_skips_missing_routed_group() {
        let rules = vec![test_rule("TPD_A", "OP_A"), test_rule("TPD_B", "OP_B")];
        let groups = vec![remote_file_source::RoutedInputGroup {
            route: "TPD_A".to_string(),
            files: vec![PathBuf::from("downloads/tpd_a/a.csv.gz")],
        }];

        let tasks = build_streaming_table_tasks(&rules, &groups, &[]).unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].dest_table, "TPD_A");
    }

    #[test]
    fn build_streaming_table_tasks_uses_fallback_inputs_without_groups() {
        let rules = vec![test_rule("TPD_A", "OP_A"), test_rule("TPD_B", "OP_B")];
        let fallback_inputs = vec![PathBuf::from("local/a.csv.gz")];

        let tasks = build_streaming_table_tasks(&rules, &[], &fallback_inputs).unwrap();

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].inputs, fallback_inputs);
        assert_eq!(tasks[1].inputs, vec![PathBuf::from("local/a.csv.gz")]);
    }

    #[test]
    fn effective_streaming_parallelism_uses_task_count() {
        assert_eq!(effective_streaming_parallelism(0), 0);
        assert_eq!(effective_streaming_parallelism(1), 1);
        assert_eq!(effective_streaming_parallelism(3), 3);
    }
}
