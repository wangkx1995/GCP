use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{Local, NaiveDateTime};

use crate::config::MappingConfig;
use crate::load_config::LoadConfig;
use crate::{LoadType, Row, TableRows};

pub fn write_tables(
    mapping: &MappingConfig,
    tables: &TableRows,
    output_dir: &Path,
    delimiter: u8,
    collect_id: &str,
    load_type: LoadType,
    load_config: &LoadConfig,
) -> Result<()> {
    let options = WriteOptions {
        output_dir,
        delimiter,
        collect_id,
        load_type,
        load_config,
    };
    fs::create_dir_all(output_dir)?;
    for (table, rows) in tables {
        if table.starts_with("OP_") {
            eprintln!("[write] SKIP {} ({} rows)", table, rows.len());
            continue;
        }
        let t = std::time::Instant::now();
        let headers = mapping
            .headers
            .get(table)
            .cloned()
            .unwrap_or_else(|| infer_headers(rows));
        let groups = group_rows_by_scan_start(rows)?;

        for (scan_start, group_rows) in groups {
            write_package(&options, table, &headers, &group_rows, &scan_start)?;
        }

        eprintln!(
            "[write] {} ({} rows, {} package(s), {:.2}s)",
            table,
            rows.len(),
            rows.iter()
                .filter_map(|row| row.get("scan_start_time"))
                .collect::<HashSet<_>>()
                .len(),
            t.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

struct WriteOptions<'a> {
    output_dir: &'a Path,
    delimiter: u8,
    collect_id: &'a str,
    load_type: LoadType,
    load_config: &'a LoadConfig,
}

fn write_package(
    options: &WriteOptions<'_>,
    table: &str,
    headers: &[String],
    rows: &[&Row],
    scan_start: &ScanStart,
) -> Result<()> {
    let table_lower = table.to_ascii_lowercase();
    let table_dir = options
        .output_dir
        .join(format!("{}_{}", table_lower, scan_start.hour_key));
    let package_dir = table_dir.join(format!("{}_{}", options.collect_id, scan_start.minute_key));
    fs::create_dir_all(&package_dir)?;

    let csv_name = format!("{}.csv", table_lower);
    let ini_name = format!("{}.ini", table_lower);
    let csv_path = package_dir.join(&csv_name);
    let ini_path = package_dir.join(&ini_name);
    let ctl_path = package_dir.join("load.ctl");
    let result_path = package_dir.join("result.csv");

    write_csv(&csv_path, headers, rows, options.delimiter)?;
    write_ini(&ini_path, headers)?;
    write_load_ctl(
        &ctl_path,
        table,
        headers,
        &csv_name,
        options.delimiter,
        options.load_type,
        options.load_config,
    )?;
    write_result_csv(&result_path, table, &scan_start.value, rows.len())?;

    eprintln!(
        "[write] {} -> {} ({} rows)",
        table,
        package_dir.display(),
        rows.len()
    );
    Ok(())
}

fn write_csv(path: &Path, headers: &[String], rows: &[&Row], delimiter: u8) -> Result<()> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .from_path(path)?;
    let mut record = Vec::with_capacity(headers.len());
    for row in rows {
        record.clear();
        for header in headers {
            record.push(row.get(header).map(String::as_str).unwrap_or_default());
        }
        writer.write_record(&record)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_ini(path: &Path, headers: &[String]) -> Result<()> {
    fs::write(path, format!("{}\n", headers.join("\n")))?;
    Ok(())
}

fn write_load_ctl(
    path: &Path,
    table: &str,
    headers: &[String],
    csv_name: &str,
    delimiter: u8,
    load_type: LoadType,
    load_config: &LoadConfig,
) -> Result<()> {
    let delimiter = delimiter as char;
    let columns = headers.join(",");
    let text = match load_type {
        LoadType::Clickhouse => {
            let cfg = &load_config.clickhouse;
            let table_name = format_table_name(table, &cfg.table_name_case);
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\n\n{client} --host {host:?} --port {port} --user {user:?} --password {password:?} --database {database:?} --query {query:?} < {csv_name:?}\n",
                client = shell_word(&cfg.client),
                host = cfg.host,
                port = cfg.port,
                user = cfg.user,
                password = cfg.password,
                database = cfg.database,
                query = format!(
                    "INSERT INTO {} ({}) SETTINGS format_csv_delimiter='{}' FORMAT CSV",
                    table_name, columns, delimiter
                ),
                csv_name = csv_name,
            )
        }
        LoadType::Postgresql => {
            let cfg = &load_config.postgresql;
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\n\nPGPASSWORD={password:?} {client} --host {host:?} --port {port} --username {user:?} --dbname {database:?} --command {command:?}\n",
                password = cfg.password,
                client = shell_word(&cfg.client),
                host = cfg.host,
                port = cfg.port,
                user = cfg.user,
                database = cfg.database,
                command = format!(
                    "\\copy {} ({}) FROM '{}' WITH (FORMAT csv, DELIMITER '{}')",
                    table, columns, csv_name, delimiter
                ),
            )
        }
    };
    fs::write(path, text)?;
    set_executable(path)?;
    Ok(())
}

fn write_result_csv(path: &Path, table: &str, data_time: &str, row_count: usize) -> Result<()> {
    let collect_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let row_count = row_count.to_string();
    let mut writer = csv::WriterBuilder::new().from_path(path)?;
    writer.write_record([
        "table_name",
        "data_time",
        "row_count",
        "success",
        "collect_time",
    ])?;
    writer.write_record([table, data_time, &row_count, "1", &collect_time])?;
    writer.flush()?;
    Ok(())
}

fn format_table_name(table: &str, table_name_case: &str) -> String {
    if table_name_case.eq_ignore_ascii_case("lower") {
        table.to_ascii_lowercase()
    } else {
        table.to_ascii_uppercase()
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn shell_word(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || "-_/.".contains(ch) {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn group_rows_by_scan_start(rows: &[Row]) -> Result<Vec<(ScanStart, Vec<&Row>)>> {
    let mut grouped: HashMap<String, Vec<&Row>> = HashMap::new();
    for row in rows {
        let value = row
            .get("scan_start_time")
            .context("output row missing scan_start_time")?;
        grouped.entry(value.clone()).or_default().push(row);
    }

    let mut result = Vec::with_capacity(grouped.len());
    for (value, rows) in grouped {
        result.push((parse_scan_start(&value)?, rows));
    }
    result.sort_by(|left, right| left.0.minute_key.cmp(&right.0.minute_key));
    Ok(result)
}

fn parse_scan_start(value: &str) -> Result<ScanStart> {
    let parsed = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .with_context(|| format!("invalid scan_start_time: {value}"))?;
    Ok(ScanStart {
        value: value.to_string(),
        hour_key: parsed.format("%Y%m%d%H").to_string(),
        minute_key: parsed.format("%Y%m%d%H%M").to_string(),
    })
}

struct ScanStart {
    value: String,
    hour_key: String,
    minute_key: String,
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
