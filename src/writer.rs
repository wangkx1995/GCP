use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::config::MappingConfig;
use crate::{Row, TableRows};

pub fn write_tables(
    mapping: &MappingConfig,
    tables: &TableRows,
    output_dir: &Path,
    delimiter: u8,
) -> Result<()> {
    fs::create_dir_all(output_dir)?;
    for (table, rows) in tables {
        if table.starts_with("OP_") {
            eprintln!("[write] SKIP {} ({} rows)", table, rows.len());
            continue;
        }
        let t = std::time::Instant::now();
        let output_path = output_dir.join(format!("{}.csv", table.to_ascii_uppercase()));
        let mut writer = csv::WriterBuilder::new()
            .delimiter(delimiter)
            .from_path(&output_path)?;
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
                record.push(row.get(header).map(String::as_str).unwrap_or_default());
            }
            writer.write_record(&record)?;
        }
        writer.flush()?;
        eprintln!(
            "[write] {} -> {} ({} rows, {:.2}s)",
            table,
            output_path.display(),
            rows.len(),
            t.elapsed().as_secs_f64()
        );
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
