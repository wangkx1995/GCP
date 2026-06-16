use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use chrono::Local;
use clap::Parser;
use encoding_rs::GBK;
use flate2::read::GzDecoder;
use indexmap::IndexMap;
use serde::Deserialize;
use tempfile::TempDir;
use walkdir::WalkDir;
use zip::ZipArchive;

mod crc64;

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
    #[arg(long, default_value = "UTF-8")]
    encoding: String,
    #[arg(long)]
    recursive: bool,
    #[arg(long = "rule-file")]
    rule_files: Vec<PathBuf>,
    #[arg(long = "rules-dir")]
    rules_dir: Option<PathBuf>,
}

struct MappingConfig {
    table_mapping: HashMap<String, String>,
    columns: IndexMap<String, IndexMap<String, String>>,
    headers: HashMap<String, Vec<String>>,
    filenum: i32,
}

struct ContextData {
    mapping: MappingConfig,
    encoding: String,
}

#[derive(Debug, Deserialize)]
struct TpdRule {
    table_name: String,
    groups: Vec<GroupRule>,
    temp_fields: Vec<FieldRule>,
    output_fields: Vec<FieldRule>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroupRule {
    name: String,
    #[serde(default)]
    group_name: String,
    #[serde(default)]
    enabled: bool,
    source_table: String,
    #[serde(default)]
    where_expr: String,
    #[serde(default)]
    group_by: Vec<String>,
    #[serde(default)]
    order_by: Vec<String>,
    #[serde(default)]
    join_keys: Vec<String>,
    #[serde(default)]
    transform_type: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct FieldRule {
    name: String,
    #[serde(default)]
    field_cn: String,
    #[serde(default)]
    field_eng: String,
    #[serde(default)]
    data_type: String,
    #[serde(default)]
    constraint: String,
    #[serde(default)]
    default_value: String,
    #[serde(default)]
    expression: String,
    #[serde(default)]
    related_group: String,
    #[serde(default)]
    description: String,
}

fn main() -> Result<()> {
    let start = Instant::now();
    let cli = Cli::parse();
    let mapping_path = cli.config_dir.join("mapping_dx.ini");
    let mapping = parse_mapping_config(&mapping_path)
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
        parse_path(&ctx, input, temp_dir.path(), &mut tables)
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
        let rule = load_rule(rule_file)?;
        execute_tpd_rule(&rule, &mut tables)
            .with_context(|| format!("failed to execute rule {}", rule_file.display()))?;
    }

    write_tables(&ctx.mapping, &tables, &cli.output_dir)?;
    eprintln!("[done] {:.2}s total", start.elapsed().as_secs_f64());
    Ok(())
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

fn parse_path(
    ctx: &ContextData,
    path: &Path,
    temp_root: &Path,
    tables: &mut TableRows,
) -> Result<()> {
    let lower_name = file_name(path).to_ascii_lowercase();
    eprintln!("[parse] {} ...", path.display());

    if lower_name.ends_with(".gz") {
        let out_path = temp_root.join(strip_suffix(&file_name(path), ".gz"));
        let mut decoder = GzDecoder::new(File::open(path)?);
        let mut out = File::create(&out_path)?;
        std::io::copy(&mut decoder, &mut out)?;
        return parse_path(ctx, &out_path, temp_root, tables);
    }

    if lower_name.ends_with(".zip") {
        let zip_dir = temp_root.join(format!("zip_{}", sanitize_file_name(&file_name(path))));
        fs::create_dir_all(&zip_dir)?;
        let mut archive = ZipArchive::new(File::open(path)?)?;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            if file.is_dir() {
                continue;
            }
            let out_path = zip_dir.join(sanitize_file_name(file.name()));
            let mut out = File::create(&out_path)?;
            std::io::copy(&mut file, &mut out)?;
            parse_path(ctx, &out_path, temp_root, tables)?;
        }
        return Ok(());
    }

    if lower_name.ends_with(".csv") || looks_like_delimited(path)? {
        return parse_csv(ctx, path, tables);
    }

    if lower_name.ends_with(".xml") {
        bail!("XML parsing is not implemented yet");
    }

    Ok(())
}

fn parse_csv(ctx: &ContextData, path: &Path, tables: &mut TableRows) -> Result<()> {
    let t = Instant::now();
    let content = read_text(path, &ctx.encoding)?;
    let name = file_name(path);

    let (delimiter, filenum) = if name.starts_with("EastCom_PM_OR") {
        (b',', 0)
    } else {
        (b'|', ctx.mapping.filenum)
    };

    let counter = detect_counter_from_filename(path, &ctx.mapping)?;
    let table = resolve_table(&ctx.mapping, &counter, path)?;
    let columns = ctx
        .mapping
        .columns
        .get(&table)
        .with_context(|| format!("missing column mapping for table {table}"))?;

    if filenum == -1 {
        let field_pairs: Vec<String> = columns.values().cloned().collect();
        for line in content.lines() {
            let values: Vec<&str> = line.split(delimiter as char).collect();
            let mut row = Row::new();
            for (idx, target_field) in field_pairs.iter().enumerate() {
                let val = values
                    .get(idx)
                    .map(|v| normalize_value(v))
                    .unwrap_or_default();
                row.insert(target_field.clone(), val);
            }
            enrich_row(&mut row, path, &HashMap::new());
            tables.entry(table.clone()).or_default().push(row);
        }
    } else {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(true)
            .flexible(true)
            .from_reader(content.as_bytes());

        let header_record = reader.headers()?.clone();
        let headers: Vec<String> = header_record.iter().map(normalize_lookup_name).collect();

        for record in reader.records() {
            let record = record?;
            let mut source = HashMap::new();
            for (idx, value) in record.iter().enumerate() {
                if let Some(header) = headers.get(idx) {
                    source.insert(header.clone(), normalize_value(value));
                }
            }

            let mut row = Row::new();
            for (source_name, target_name) in columns {
                let value = lookup_source_value(&source, source_name, target_name);
                row.insert(target_name.clone(), value);
            }
            enrich_row(&mut row, path, &source);
            tables.entry(table.clone()).or_default().push(row);
        }
    }

    let rows = tables.get(&table).map(|r| r.len()).unwrap_or(0);
    let cols = columns.len();
    eprintln!("  -> table={table} rows={rows} cols={cols} counter={counter} filenum={filenum} ({:.2}s)", t.elapsed().as_secs_f64());
    Ok(())
}

fn parse_mapping_config(path: &Path) -> Result<MappingConfig> {
    let text = read_config_text(path)?;
    let mut section = String::new();
    let mut table_mapping = HashMap::new();
    let mut columns: IndexMap<String, IndexMap<String, String>> = IndexMap::new();
    let mut headers: HashMap<String, Vec<String>> = HashMap::new();
    let mut filenum = -1;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(&['[', ']'][..]).to_ascii_lowercase();
            continue;
        }

        match section.as_str() {
            "tablemapping" => {
                let parts = split_mapping_line(line);
                if parts.len() >= 2 {
                    if parts[0].to_ascii_lowercase() == "filenum" {
                        filenum = parts[1].parse::<i32>().unwrap_or(-1);
                    } else {
                        table_mapping
                            .insert(parts[0].to_ascii_uppercase(), parts[1].to_ascii_uppercase());
                    }
                }
            }
            "colnamemapping" => {
                let parts = split_mapping_line(line);
                if parts.len() >= 3 {
                    let table = parts[0].to_ascii_uppercase();
                    let source = parts[1].to_string();
                    let target = parts[2].to_string();
                    columns
                        .entry(table.clone())
                        .or_default()
                        .insert(source, target.clone());
                    headers.entry(table).or_default().push(target);
                }
            }
            _ => {}
        }
    }

    Ok(MappingConfig {
        table_mapping,
        columns,
        headers,
        filenum,
    })
}

fn read_config_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(err) => {
            let bytes = err.into_bytes();
            let (text, _, _) = GBK.decode(&bytes);
            Ok(text.into_owned())
        }
    }
}

fn split_mapping_line(line: &str) -> Vec<&str> {
    line.split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn load_rule(path: &Path) -> Result<TpdRule> {
    let text = fs::read_to_string(path)?;
    let rule = serde_json::from_str(&text)?;
    Ok(rule)
}

fn execute_tpd_rule(rule: &TpdRule, tables: &mut TableRows) -> Result<()> {
    let t = Instant::now();
    let Some(group) = rule.groups.iter().find(|group| group.enabled) else {
        bail!("rule {} does not contain an enabled group", rule.table_name);
    };
    let source_key = group.source_table.to_ascii_uppercase();
    let source_rows = match tables.get(&source_key).or_else(|| tables.get(&group.source_table)) {
        Some(rows) => rows,
        None => {
            let available: Vec<&String> = tables.keys().collect();
            eprintln!(
                "[aggregate] SKIP {} <- {}: source table not found. Available tables: {:?}",
                rule.table_name, group.source_table, available,
            );
            return Ok(());
        }
    };

    eprintln!(
        "[aggregate] {} <- {} ({} source rows, group by {:?})",
        rule.table_name,
        group.source_table,
        source_rows.len(),
        group.group_by,
    );

    let mut grouped: IndexMap<String, Vec<&Row>> = IndexMap::new();
    for row in source_rows {
        let key = group
            .group_by
            .iter()
            .map(|field| get_row_value(row, field))
            .collect::<Vec<_>>()
            .join("\u{1f}");
        grouped.entry(key).or_default().push(row);
    }

    let mut output_rows = Vec::new();
    for rows in grouped.values() {
        let mut context = Row::new();
        for field in &group.group_by {
            context.insert(field.clone(), get_row_value(rows[0], field));
        }

        for field in &rule.temp_fields {
            let value = eval_expression(&field.expression, rows, &context);
            context.insert(field.name.trim().to_string(), value);
        }

        let mut output = Row::new();
        for field in &rule.output_fields {
            let value = eval_expression(&field.expression, rows, &merge_context(&context, &output));
            output.insert(field.name.trim().to_string(), value);
        }
        output_rows.push(output);
    }

    eprintln!(
        "  -> {} groups -> {} output rows ({:.2}s)",
        grouped.len(),
        output_rows.len(),
        t.elapsed().as_secs_f64(),
    );
    tables.insert(rule.table_name.to_ascii_uppercase(), output_rows);
    Ok(())
}

fn merge_context(left: &Row, right: &Row) -> Row {
    let mut merged = left.clone();
    for (key, value) in right {
        merged.insert(key.clone(), value.clone());
    }
    merged
}

fn eval_expression(expr: &str, rows: &[&Row], context: &Row) -> String {
    let expr = expr.trim();
    if expr.is_empty() {
        return String::new();
    }
    if expr.parse::<f64>().is_ok() {
        return expr.to_string();
    }
    let lower = expr.to_ascii_lowercase();

    if lower == "current_timestamp" {
        return Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    }
    if let Some(value) = parse_quoted_env(expr) {
        return value;
    }
    if lower.starts_with("max(") && expr.ends_with(')') {
        let inner = &expr[4..expr.len() - 1];
        return max_value(rows, inner);
    }
    if lower.starts_with("count(distinct ") && expr.ends_with(')') {
        let inner = expr[15..expr.len() - 1].trim();
        let mut values = HashSet::new();
        for row in rows {
            values.insert(get_row_value(row, inner));
        }
        return values.len().to_string();
    }
    if lower.starts_with("crc64(") && expr.ends_with(')') {
        let inner = &expr[6..expr.len() - 1];

        let value = eval_expression(inner, rows, context);
        return crate::crc64::crc64_ecma(&value).to_string();
    }
    if lower.starts_with("case when ") {
        return eval_case_when(expr, rows, context);
    }
    if expr.contains("||") {
        return expr
            .split("||")
            .map(|part| eval_concat_part(part, rows, context))
            .collect::<Vec<_>>()
            .join("");
    }
    if let Some(value) = parse_quoted_literal(expr) {
        return value;
    }
    if let Some(value) = get_context_value(context, expr) {
        return value;
    }
    get_row_value(rows[0], expr)
}

fn eval_concat_part(part: &str, rows: &[&Row], context: &Row) -> String {
    let part = part.trim();
    if let Some(value) = parse_quoted_literal(part) {
        return value;
    }
    if let Some(value) = parse_quoted_env(part) {
        return value;
    }
    if let Some(value) = get_context_value(context, part) {
        return value;
    }
    get_row_value(rows[0], part)
}

fn eval_case_when(expr: &str, rows: &[&Row], context: &Row) -> String {
    let lower = expr.to_ascii_lowercase();
    let Some(then_idx) = lower.find(" then ") else {
        return String::new();
    };
    let Some(else_idx) = lower.find(" else ") else {
        return String::new();
    };
    let Some(end_idx) = lower.rfind(" end") else {
        return String::new();
    };
    let condition = expr[10..then_idx].trim();
    let then_expr = expr[then_idx + 6..else_idx].trim();
    let else_expr = expr[else_idx + 6..end_idx].trim();
    if eval_condition(condition, context) {
        eval_expression(then_expr, rows, context)
    } else {
        eval_expression(else_expr, rows, context)
    }
}

fn eval_condition(condition: &str, context: &Row) -> bool {
    if let Some((left, right)) = condition.split_once('>') {
        let left_value = get_context_value(context, left.trim()).unwrap_or_default();
        let right_value = right.trim().parse::<f64>().unwrap_or(0.0);
        return left_value.parse::<f64>().unwrap_or(0.0) > right_value;
    }
    false
}

fn max_value(rows: &[&Row], field: &str) -> String {
    let mut best = String::new();
    let mut best_number: Option<f64> = None;
    for row in rows {
        let value = get_row_value(row, field);
        if value.is_empty() {
            continue;
        }
        if let Ok(number) = value.parse::<f64>() {
            if best_number.map_or(true, |best| number > best) {
                best_number = Some(number);
                best = value;
            }
        } else if best_number.is_none() && value > best {
            best = value;
        }
    }
    best
}

fn get_row_value(row: &Row, field: &str) -> String {
    if let Some(value) = row.get(field) {
        return value.clone();
    }
    let normalized = normalize_lookup_name(field);
    for (key, value) in row {
        if normalize_lookup_name(key) == normalized {
            return value.clone();
        }
    }
    String::new()
}

fn get_context_value(context: &Row, field: &str) -> Option<String> {
    if let Some(value) = context.get(field) {
        return Some(value.clone());
    }
    let normalized = normalize_lookup_name(field);
    for (key, value) in context {
        if normalize_lookup_name(key) == normalized {
            return Some(value.clone());
        }
    }
    None
}

fn parse_quoted_literal(expr: &str) -> Option<String> {
    let expr = expr.trim();
    if expr.len() >= 2 && expr.starts_with('\'') && expr.ends_with('\'') {
        return Some(expr[1..expr.len() - 1].to_string());
    }
    None
}

fn parse_quoted_env(expr: &str) -> Option<String> {
    let literal = parse_quoted_literal(expr)?;
    if literal.starts_with("${") && literal.ends_with('}') {
        let key = &literal[2..literal.len() - 1];
        return Some(std::env::var(key).unwrap_or_default());
    }
    None
}

fn resolve_table(mapping: &MappingConfig, counter: &str, path: &Path) -> Result<String> {
    let counter_upper = counter.to_ascii_uppercase();
    let table = mapping
        .table_mapping
        .get(&counter_upper)
        .cloned()
        .with_context(|| format!("missing table mapping for counter {counter_upper}"))?;

    let name = file_name(path).to_ascii_uppercase();
    if name.contains("-NSA-") || name.contains("_NSA_") {
        let nsa_key = format!("{table}_NSA");
        if let Some(nsa_table) = mapping.table_mapping.get(&nsa_key) {
            return Ok(nsa_table.clone());
        }
    } else if name.contains("-SA-") || name.contains("_SA_") {
        let sa_key = format!("{table}_SA");
        if let Some(sa_table) = mapping.table_mapping.get(&sa_key) {
            return Ok(sa_table.clone());
        }
    }
    Ok(table)
}

fn detect_counter_from_filename(path: &Path, mapping: &MappingConfig) -> Result<String> {
    let name = file_name(path).to_ascii_uppercase();
    let mut keys: Vec<&String> = mapping.table_mapping.keys().collect();
    keys.sort_by_key(|key| std::cmp::Reverse(key.len()));
    for key in keys {
        if key == "FILENUM" || key.starts_with("OP_") {
            continue;
        }
        if name.contains(key) {
            return Ok(key.clone());
        }
    }
    bail!("could not determine counter from {}", path.display())
}

fn lookup_source_value(
    source: &HashMap<String, String>,
    source_name: &str,
    target_name: &str,
) -> String {
    let mut candidates = vec![
        normalize_lookup_name(source_name),
        normalize_lookup_name(target_name),
        normalize_lookup_name(&column_name_format(source_name)),
        normalize_lookup_name(&column_name_format(target_name)),
    ];

    match normalize_lookup_name(source_name).as_str() {
        "RMUID" => candidates.push("RMUID".to_string()),
        "DN" => candidates.push("DN".to_string()),
        "STARTTIME" => candidates.push("BEGINTIME".to_string()),
        "ENDTIME" => candidates.push("ENDTIME".to_string()),
        "USERLABEL" => candidates.push("RDN".to_string()),
        "HO_ATTOUTPERRELATION_CAUSECOVERING" => {
            candidates.push("HO_ATTOUTPERRELATION_CAUSE".to_string())
        }
        _ => {}
    }

    let mut seen = HashSet::new();
    for candidate in candidates {
        if seen.insert(candidate.clone()) {
            if let Some(value) = source.get(&candidate) {
                return value.clone();
            }
        }
    }
    String::new()
}

fn enrich_row(row: &mut Row, path: &Path, source: &HashMap<String, String>) {
    let name = file_name(path).to_ascii_uppercase();
    if let Some(value) = source.get("IS_NSA") {
        row.insert("is_nsa".to_string(), value.clone());
    } else if name.contains("-NSA-") || name.contains("_NSA_") {
        row.insert("is_nsa".to_string(), "1".to_string());
    } else if name.contains("-SA-") || name.contains("_SA_") {
        row.insert("is_nsa".to_string(), "0".to_string());
    }

    let dn = row
        .get("Dn")
        .or_else(|| row.get("dn"))
        .cloned()
        .or_else(|| source.get("DN").cloned())
        .unwrap_or_default();
    if !dn.is_empty() {
        split_dn(row, &dn);
    }
}

fn split_dn(row: &mut Row, dn: &str) {
    for part in dn.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        row.entry(key.to_string())
            .or_insert_with(|| value.to_string());
        row.entry(key.to_ascii_lowercase())
            .or_insert_with(|| value.to_string());
    }
    if let Some(idx) = dn.rfind(',') {
        row.entry("parent_dn".to_string())
            .or_insert_with(|| dn[..idx].to_string());
    }
}

fn write_tables(mapping: &MappingConfig, tables: &TableRows, output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)?;
    for (table, rows) in tables {
        if table.starts_with("OP_") {
            eprintln!("[write] SKIP {} ({} rows)", table, rows.len());
            continue;
        }
        let t = Instant::now();
        let output_path = output_dir.join(format!("{}.csv", table.to_ascii_uppercase()));
        let mut writer = csv::WriterBuilder::new().from_path(&output_path)?;
        let headers = mapping
            .headers
            .get(table)
            .cloned()
            .unwrap_or_else(|| infer_headers(rows));
        writer.write_record(&headers)?;
        let mut record = Vec::with_capacity(headers.len());
        for row in rows {
            record.clear();
            for header in &headers {
                record.push(row.get(header).cloned().unwrap_or_default());
            }
            writer.write_record(&record)?;
        }
        writer.flush()?;
        eprintln!("[write] {} -> {} ({} rows, {:.2}s)", table, output_path.display(), rows.len(), t.elapsed().as_secs_f64());
    }
    Ok(())
}

fn infer_headers(rows: &[Row]) -> Vec<String> {
    let mut headers = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        for key in row.keys() {
            if seen.insert(key.clone()) {
                headers.push(key.clone());
            }
        }
    }
    headers
}

fn read_text(path: &Path, encoding: &str) -> Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    if encoding.eq_ignore_ascii_case("GBK") || encoding.eq_ignore_ascii_case("GB2312") {
        let (text, _, _) = GBK.decode(&bytes);
        return Ok(text.into_owned());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn looks_like_delimited(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut buf = [0_u8; 512];
    let len = file.read(&mut buf)?;
    Ok(buf[..len].contains(&b'|'))
}

fn normalize_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed == "/0" {
        return "0".to_string();
    }
    if trimmed.eq_ignore_ascii_case("NIL")
        || trimmed.eq_ignore_ascii_case("NULL")
        || trimmed == "\"\""
        || trimmed.eq_ignore_ascii_case("N/A")
        || trimmed == "-"
    {
        return String::new();
    }
    if trimmed.contains('"') {
        trimmed.replace('"', "")
    } else {
        trimmed.to_string()
    }
}

fn column_name_format(value: &str) -> String {
    value
        .trim()
        .replace("&gt;&lt;", "_")
        .replace("&gt;", "")
        .replace("&lt;", "")
        .replace("><", "_")
        .replace('>', "")
        .replace('<', "")
        .replace("][", "_")
        .replace('[', "")
        .replace(']', "")
        .replace('.', "_")
}

fn normalize_lookup_name(value: &str) -> String {
    column_name_format(value)
        .to_ascii_uppercase()
        .replace(' ', "")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn strip_suffix(value: &str, suffix: &str) -> String {
    value
        .strip_suffix(suffix)
        .or_else(|| value.strip_suffix(&suffix.to_ascii_uppercase()))
        .unwrap_or(value)
        .to_string()
}

fn sanitize_file_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch == '/' || ch == '\\' || ch == ':' {
                '_'
            } else {
                ch
            }
        })
        .collect()
}
