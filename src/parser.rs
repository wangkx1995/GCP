use std::collections::HashMap;
use std::fs::{self, File};
use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use zip::ZipArchive;

use crate::config::{
    detect_counter_from_filename, lookup_source_value, resolve_table, ContextData,
};
use crate::util::*;
use crate::{Row, TableRows};

pub fn parse_path(
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
    eprintln!(
        "  -> table={table} rows={rows} cols={cols} counter={counter} filenum={filenum} ({:.2}s)",
        t.elapsed().as_secs_f64()
    );
    Ok(())
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