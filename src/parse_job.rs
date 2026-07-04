use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use tempfile::TempDir;
use tracing::{info, warn};

use crate::config::ContextData;
use crate::load_config::LoadConfig;
use crate::tpd;
use crate::TableRows;
use crate::LoadType;

#[derive(Clone, Debug)]
pub struct ParseJobOptions {
    pub input: Option<PathBuf>,
    pub source_config: Option<PathBuf>,
    pub scan_start_time: Option<String>,
    pub config_dir: PathBuf,
    pub output_dir: PathBuf,
    pub collect_id: String,
    pub load_type: LoadType,
    pub load_config: PathBuf,
    pub output_delimiter: String,
    pub encoding: String,
    pub recursive: bool,
    pub rule_files: Vec<PathBuf>,
    pub rules_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseJobSummary {
    pub task_count: usize,
}

pub fn parse_delimiter(value: &str) -> Result<u8> {
    let bytes = value.as_bytes();
    if bytes.len() != 1 {
        bail!("output delimiter must be exactly one ASCII byte, got {value:?}");
    }
    Ok(bytes[0])
}

pub fn run_parse_job(options: ParseJobOptions) -> Result<ParseJobSummary> {
    let mapping_path = options.config_dir.join("mapping_dx.ini");
    let output_delimiter = parse_delimiter(&options.output_delimiter)?;
    let load_config = crate::load_config::load_config(&options.load_config)
        .with_context(|| format!("failed to parse {}", options.load_config.display()))?;
    let mapping = crate::config::parse_mapping_config(&mapping_path)
        .with_context(|| format!("failed to parse {}", mapping_path.display()))?;
    let ctx = ContextData {
        mapping,
        encoding: options.encoding,
    };

    let rule_files = discover_rule_files(options.rule_files, options.rules_dir.as_ref())?;
    let mut rules = Vec::new();
    for rule_file in &rule_files {
        tracing::info!("[rule] loading {}", rule_file.display());
        rules.push(crate::tpd::load_rule(rule_file)?);
    }
    crate::tpd::validate_streaming_rules(&rules)?;
    let dest_tables_by_source = dest_tables_by_source_table(&rules);

    let source_config = match &options.source_config {
        Some(path) => Some(remote_file_source::config::load_source_config(path)?),
        None => None,
    };
    let routed_inputs = remote_file_source::resolve_routed_files_with_router(
        remote_file_source::ResolveOptions {
            local_input: options.input,
            recursive: options.recursive,
            source_config,
            scan_start_time: options.scan_start_time,
        },
        |remote_file| route_remote_file(remote_file, &ctx, &dest_tables_by_source),
    )?;
    let tasks = build_streaming_table_tasks(
        &rules,
        &routed_inputs.groups,
        &routed_inputs.representative_files,
    )?;
    let task_count = tasks.len();
    run_streaming_table_tasks(
        tasks,
        &ctx,
        &options.output_dir,
        output_delimiter,
        &options.collect_id,
        options.load_type,
        &load_config,
    )?;

    Ok(ParseJobSummary { task_count })
}

pub fn cleanup_old_logs(dir: &Path, retention_days: u64) -> Result<()> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(retention_days * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            if let Ok(modified) = entry.metadata()?.modified() {
                if modified < cutoff {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
    Ok(())
}

struct StreamingTableTask {
    dest_table: String,
    rules: Vec<tpd::TpdRule>,
    inputs: Vec<PathBuf>,
}

fn run_streaming_table_task(
    task: StreamingTableTask,
    ctx: &ContextData,
    output_dir: &Path,
    output_delimiter: u8,
    collect_id: &str,
    load_type: LoadType,
    load_config: &LoadConfig,
) -> Result<()> {
    let temp_dir = TempDir::new().context("failed to create temp dir")?;
    let streaming_required_fields = tpd::streaming_required_fields_by_table(&task.rules);
    let streaming_ordered_fields = tpd::streaming_ordered_fields_by_table(&task.rules);
    let streaming_engine = RefCell::new(tpd::StreamingTpdEngine::new(&task.rules));
    info!(
        "[aggregate] {} input file(s) for {}",
        task.inputs.len(),
        task.dest_table
    );
    for input in &task.inputs {
        crate::parser::parse_path_with_streaming_values(
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
    output_dir: &Path,
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
            warn!("[rule] no .json files found in {}", rules_dir.display());
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
    let Ok(counter) = crate::config::detect_counter_from_filename(&path, &ctx.mapping) else {
        return Vec::new();
    };
    let Ok(source_table) = crate::config::resolve_table(&ctx.mapping, &counter, &path) else {
        return Vec::new();
    };
    dest_tables_by_source
        .get(&source_table.to_ascii_uppercase())
        .cloned()
        .unwrap_or_default()
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
                warn!("[input] skip {dest_table}: no routed input files");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_delimiter_accepts_one_ascii_byte() {
        assert_eq!(parse_delimiter("|").unwrap(), b'|');
        assert_eq!(parse_delimiter(",").unwrap(), b',');
    }

    #[test]
    fn parse_delimiter_rejects_empty_or_multi_byte_values() {
        assert!(parse_delimiter("").unwrap_err().to_string().contains("output delimiter"));
        assert!(parse_delimiter("||").unwrap_err().to_string().contains("output delimiter"));
        assert!(parse_delimiter("中").unwrap_err().to_string().contains("output delimiter"));
    }

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
