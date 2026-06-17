use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use indexmap::IndexMap;
use tempfile::TempDir;
use walkdir::WalkDir;

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
    input: PathBuf,
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

    let temp_dir = TempDir::new().context("failed to create temp dir")?;
    let mut tables = TableRows::new();
    let inputs = collect_inputs(&cli.input, cli.recursive)?;
    eprintln!("[input] {} file(s) to process", inputs.len());
    for input in &inputs {
        parser::parse_path(&ctx, input, temp_dir.path(), &mut tables)
            .with_context(|| format!("failed to parse {}", input.display()))?;
    }

    let mut rule_files: Vec<PathBuf> = cli.rule_files;
    if let Some(rules_dir) = &cli.rules_dir {
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

    for rule_file in &rule_files {
        eprintln!("[rule] loading {}", rule_file.display());
        let rule = tpd::load_rule(rule_file)?;
        tpd::execute_tpd_rule(&rule, &mut tables)
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

fn parse_delimiter(value: &str) -> Result<u8> {
    let bytes = value.as_bytes();
    if bytes.len() != 1 {
        bail!("output delimiter must be exactly one ASCII byte, got {value:?}");
    }
    Ok(bytes[0])
}

fn collect_inputs(input: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        return Ok(vec![input.to_path_buf()]);
    }
    if !input.is_dir() {
        bail!("input does not exist: {}", input.display());
    }

    let mut files = Vec::new();
    if recursive {
        for entry in WalkDir::new(input) {
            let entry = entry?;
            if entry.file_type().is_file() {
                files.push(entry.path().to_path_buf());
            }
        }
    } else {
        for entry in fs::read_dir(input)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}
