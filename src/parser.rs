use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use chrono::NaiveDateTime;
use flate2::read::GzDecoder;
use zip::ZipArchive;

use crate::config::{detect_counter_from_filename, resolve_table, ContextData};
use crate::util::*;
use crate::{Row, TableRows};

#[allow(dead_code)]
pub fn parse_path(
    ctx: &ContextData,
    path: &Path,
    temp_root: &Path,
    tables: &mut TableRows,
) -> Result<()> {
    parse_path_with_handler(ctx, path, temp_root, &mut |table, row| {
        tables.entry(table.to_string()).or_default().push(row);
        Ok(())
    })
}

pub fn parse_path_with_handler<F>(
    ctx: &ContextData,
    path: &Path,
    temp_root: &Path,
    handler: &mut F,
) -> Result<()>
where
    F: FnMut(&str, Row) -> Result<()>,
{
    parse_path_with_projection(ctx, path, temp_root, &HashMap::new(), handler)
}

pub fn parse_path_with_projection<F>(
    ctx: &ContextData,
    path: &Path,
    temp_root: &Path,
    projections: &HashMap<String, HashSet<String>>,
    handler: &mut F,
) -> Result<()>
where
    F: FnMut(&str, Row) -> Result<()>,
{
    let lower_name = file_name(path).to_ascii_lowercase();
    eprintln!("[parse] {} ...", path.display());

    if lower_name.ends_with(".gz") {
        let out_path = temp_root.join(strip_suffix(&file_name(path), ".gz"));
        let mut decoder = GzDecoder::new(File::open(path)?);
        let mut out = File::create(&out_path)?;
        std::io::copy(&mut decoder, &mut out)?;
        return parse_path_with_projection(ctx, &out_path, temp_root, projections, handler);
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
            parse_path_with_projection(ctx, &out_path, temp_root, projections, handler)?;
        }
        return Ok(());
    }

    if lower_name.ends_with(".csv") || looks_like_delimited(path)? {
        return parse_csv(ctx, path, projections, handler);
    }

    if lower_name.ends_with(".xml") {
        bail!("XML parsing is not implemented yet");
    }

    Ok(())
}

pub fn parse_path_with_streaming_values<F, G>(
    ctx: &ContextData,
    path: &Path,
    temp_root: &Path,
    projections: &HashMap<String, HashSet<String>>,
    value_schemas: &HashMap<String, Vec<String>>,
    row_handler: &mut F,
    value_handler: &mut G,
) -> Result<()>
where
    F: FnMut(&str, Row) -> Result<()>,
    G: FnMut(&str, Vec<String>) -> Result<()>,
{
    let lower_name = file_name(path).to_ascii_lowercase();
    if lower_name.ends_with(".gz") {
        let out_name = lower_name.trim_end_matches(".gz");
        let out_path = temp_root.join(sanitize_file_name(out_name));
        let mut decoder = GzDecoder::new(File::open(path)?);
        let mut out = File::create(&out_path)?;
        std::io::copy(&mut decoder, &mut out)?;
        return parse_path_with_streaming_values(
            ctx,
            &out_path,
            temp_root,
            projections,
            value_schemas,
            row_handler,
            value_handler,
        );
    }
    if lower_name.ends_with(".zip") {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(file)?;
        let zip_dir = temp_root.join(format!("zip_{}", sanitize_file_name(&file_name(path))));
        fs::create_dir_all(&zip_dir)?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            if file.is_dir() {
                continue;
            }
            let out_path = zip_dir.join(sanitize_file_name(file.name()));
            let mut out = File::create(&out_path)?;
            std::io::copy(&mut file, &mut out)?;
            parse_path_with_streaming_values(
                ctx,
                &out_path,
                temp_root,
                projections,
                value_schemas,
                row_handler,
                value_handler,
            )?;
        }
        return Ok(());
    }
    if lower_name.ends_with(".csv") || looks_like_delimited(path)? {
        return parse_csv_streaming_values(
            ctx,
            path,
            projections,
            value_schemas,
            row_handler,
            value_handler,
        );
    }
    parse_path_with_projection(ctx, path, temp_root, projections, row_handler)
}

fn parse_csv<F>(
    ctx: &ContextData,
    path: &Path,
    projections: &HashMap<String, HashSet<String>>,
    handler: &mut F,
) -> Result<()>
where
    F: FnMut(&str, Row) -> Result<()>,
{
    let t = Instant::now();
    let name = file_name(path);

    let (delimiter, filenum) = if name.to_ascii_uppercase().starts_with("EASTCOM_PM_OR") {
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
    let projection = projections
        .get(&table.to_ascii_uppercase())
        .map(normalize_projection_fields);
    let projection_ref = projection.as_ref();
    let split_dn = should_split_dn(projection_ref);
    let rows = if is_utf8_encoding(&ctx.encoding) {
        parse_csv_utf8(
            path,
            delimiter,
            filenum,
            &table,
            columns,
            projection_ref,
            split_dn,
            handler,
        )?
    } else {
        let content = read_text(path, &ctx.encoding)?;
        parse_csv_text(
            path,
            &content,
            delimiter,
            filenum,
            &table,
            columns,
            projection_ref,
            split_dn,
            handler,
        )?
    };

    let cols = columns.len();
    eprintln!(
        "  -> table={table} rows={rows} cols={cols} counter={counter} filenum={filenum} ({:.2}s)",
        t.elapsed().as_secs_f64()
    );
    Ok(())
}

fn parse_csv_streaming_values<F, G>(
    ctx: &ContextData,
    path: &Path,
    projections: &HashMap<String, HashSet<String>>,
    value_schemas: &HashMap<String, Vec<String>>,
    row_handler: &mut F,
    value_handler: &mut G,
) -> Result<()>
where
    F: FnMut(&str, Row) -> Result<()>,
    G: FnMut(&str, Vec<String>) -> Result<()>,
{
    let name = file_name(path);
    let (delimiter, filenum) = if name.to_ascii_uppercase().starts_with("EASTCOM_PM_OR") {
        (b',', 0)
    } else {
        (b'|', ctx.mapping.filenum)
    };
    let counter = detect_counter_from_filename(path, &ctx.mapping)
        .with_context(|| format!("cannot detect counter for {}", path.display()))?;
    let table = resolve_table(&ctx.mapping, &counter, path)?;
    let Some(schema) = value_schemas.get(&table.to_ascii_uppercase()) else {
        return parse_csv(ctx, path, projections, row_handler);
    };
    if !is_utf8_encoding(&ctx.encoding) {
        return parse_csv(ctx, path, projections, row_handler);
    }

    let t = Instant::now();
    let columns = ctx
        .mapping
        .columns
        .get(&table)
        .with_context(|| format!("missing column mapping for table {table}"))?;
    let rows = match filenum {
        -1 => parse_position_streaming_values(
            path,
            delimiter,
            &table,
            columns,
            schema,
            value_handler,
        )?,
        0 => {
            parse_header_streaming_values(path, delimiter, &table, columns, schema, value_handler)?
        }
        _ => return parse_csv(ctx, path, projections, row_handler),
    };
    eprintln!(
        "  -> table={table} rows={rows} cols={} counter={counter} filenum={filenum} fast=values ({:.2}s)",
        columns.len(),
        t.elapsed().as_secs_f64()
    );
    Ok(())
}

fn parse_position_streaming_values<G>(
    path: &Path,
    delimiter: u8,
    table: &str,
    columns: &indexmap::IndexMap<String, String>,
    schema: &[String],
    value_handler: &mut G,
) -> Result<usize>
where
    G: FnMut(&str, Vec<String>) -> Result<()>,
{
    let fields = projected_schema_position_fields(columns, schema);
    let metadata = StreamingValueMetadata::new(path, schema);
    let dn_derived = DnDerivedValueMetadata::new(schema);
    let reader = BufReader::new(File::open(path)?);
    let mut rows = 0_usize;
    for (line_idx, line) in reader.lines().enumerate() {
        let line = line?;
        if line_idx == 0 && looks_like_header_line(&line, delimiter, columns) {
            continue;
        }
        let mut values = fill_projected_position_values(&line, delimiter, &fields, schema.len());
        metadata.apply(&mut values);
        dn_derived.apply(&mut values);
        value_handler(table, values)?;
        rows += 1;
    }
    Ok(rows)
}

fn parse_header_streaming_values<G>(
    path: &Path,
    delimiter: u8,
    table: &str,
    columns: &indexmap::IndexMap<String, String>,
    schema: &[String],
    value_handler: &mut G,
) -> Result<usize>
where
    G: FnMut(&str, Vec<String>) -> Result<()>,
{
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .flexible(true)
        .from_reader(BufReader::new(File::open(path)?));
    let header_record = reader.headers()?.clone();
    let headers: Vec<String> = header_record.iter().map(normalize_lookup_name).collect();
    let fields = projected_schema_header_fields(&headers, columns, schema);
    let metadata = StreamingValueMetadata::new(path, schema);
    let dn_derived = DnDerivedValueMetadata::new(schema);
    let mut rows = 0_usize;
    for record in reader.records() {
        let record = record?;
        let mut values = vec![String::new(); schema.len()];
        for (source_idx, schema_idx) in &fields {
            values[*schema_idx] = record
                .get(*source_idx)
                .map(normalize_value)
                .unwrap_or_default();
        }
        metadata.apply(&mut values);
        dn_derived.apply(&mut values);
        value_handler(table, values)?;
        rows += 1;
    }
    Ok(rows)
}

struct StreamingValueMetadata {
    is_nsa_idx: Option<usize>,
    filename_is_nsa: Option<String>,
    scan_start_idxs: Vec<usize>,
    scan_stop_idxs: Vec<usize>,
    filename_scan_start: Option<String>,
    filename_scan_stop: Option<String>,
}

impl StreamingValueMetadata {
    fn new(path: &Path, schema: &[String]) -> Self {
        let is_nsa_idx = schema
            .iter()
            .position(|field| field.eq_ignore_ascii_case("is_nsa"));
        let scan_start_idxs = schema
            .iter()
            .enumerate()
            .filter_map(|(idx, field)| {
                matches!(
                    normalize_lookup_name(field).as_str(),
                    "SCAN_START_TIME" | "STARTTIME" | "BEGINTIME"
                )
                .then_some(idx)
            })
            .collect();
        let scan_stop_idxs = schema
            .iter()
            .enumerate()
            .filter_map(|(idx, field)| {
                matches!(
                    normalize_lookup_name(field).as_str(),
                    "SCAN_STOP_TIME" | "ENDTIME"
                )
                .then_some(idx)
            })
            .collect();
        let (filename_scan_start, filename_scan_stop) = filename_scan_times(path);
        Self {
            is_nsa_idx,
            filename_is_nsa: filename_is_nsa_value(path),
            scan_start_idxs,
            scan_stop_idxs,
            filename_scan_start,
            filename_scan_stop,
        }
    }

    fn apply(&self, values: &mut [String]) {
        if let (Some(idx), Some(value)) = (self.is_nsa_idx, self.filename_is_nsa.as_ref()) {
            if values[idx].is_empty() {
                values[idx] = value.clone();
            }
        }
        if let Some(value) = self.filename_scan_start.as_ref() {
            for idx in &self.scan_start_idxs {
                values[*idx] = value.clone();
            }
        }
        if let Some(value) = self.filename_scan_stop.as_ref() {
            for idx in &self.scan_stop_idxs {
                values[*idx] = value.clone();
            }
        }
    }
}

struct DnDerivedValueMetadata {
    dn_idx: Option<usize>,
    part_fields: Vec<(String, usize)>,
    parent_dn_idx: Option<usize>,
}

impl DnDerivedValueMetadata {
    fn new(schema: &[String]) -> Self {
        let mut dn_idx = None;
        let mut part_fields = Vec::new();
        let mut parent_dn_idx = None;
        for (idx, field) in schema.iter().enumerate() {
            match normalize_lookup_name(field).as_str() {
                "DN" => dn_idx = Some(idx),
                "MANAGEDELEMENT" | "GNBDUFUNCTION" | "GNBCUCPFUNCTION" | "ENBFUNCTION"
                | "NRCELLDU" | "NRCELLCU" | "EUTRANCELL" => {
                    part_fields.push((normalize_lookup_name(field), idx));
                }
                "PARENT_DN" => parent_dn_idx = Some(idx),
                _ => {}
            }
        }
        Self {
            dn_idx,
            part_fields,
            parent_dn_idx,
        }
    }

    fn apply(&self, values: &mut [String]) {
        if self.part_fields.is_empty() && self.parent_dn_idx.is_none() {
            return;
        }
        let Some(dn_idx) = self.dn_idx else {
            return;
        };
        let dn = values[dn_idx].clone();
        if dn.is_empty() {
            return;
        }

        for part in dn.split(',') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            let key = normalize_lookup_name(key.trim());
            let value = value.trim();
            for (field, idx) in &self.part_fields {
                if values[*idx].is_empty() && *field == key {
                    values[*idx] = value.to_string();
                }
            }
        }

        if let (Some(idx), Some((parent, _))) = (self.parent_dn_idx, dn.rsplit_once(',')) {
            if values[idx].is_empty() {
                values[idx] = parent.trim().to_string();
            }
        }
    }
}

fn filename_is_nsa_value(path: &Path) -> Option<String> {
    let name = file_name(path).to_ascii_uppercase();
    if name.contains("-NSA-") || name.contains("_NSA_") {
        Some("1".to_string())
    } else if name.contains("-SA-") || name.contains("_SA_") {
        Some("0".to_string())
    } else {
        None
    }
}

fn filename_scan_times(path: &Path) -> (Option<String>, Option<String>) {
    let name = file_name(path);
    let mut values = Vec::new();
    let bytes = name.as_bytes();
    let mut idx = 0_usize;
    while idx + 12 <= bytes.len() {
        if idx + 14 <= bytes.len()
            && bytes[idx..idx + 14]
                .iter()
                .all(|byte| byte.is_ascii_digit())
        {
            if let Ok(parsed) = NaiveDateTime::parse_from_str(&name[idx..idx + 14], "%Y%m%d%H%M%S")
            {
                values.push(parsed.format("%Y-%m-%d %H:%M:%S").to_string());
                idx += 14;
                continue;
            }
        }
        if bytes[idx..idx + 12]
            .iter()
            .all(|byte| byte.is_ascii_digit())
        {
            let candidate = format!("{}00", &name[idx..idx + 12]);
            if let Ok(parsed) = NaiveDateTime::parse_from_str(&candidate, "%Y%m%d%H%M%S") {
                values.push(parsed.format("%Y-%m-%d %H:%M:%S").to_string());
                idx += 12;
                continue;
            }
        }
        idx += 1;
    }
    (values.first().cloned(), values.get(1).cloned())
}

fn looks_like_header_line(
    line: &str,
    delimiter: u8,
    columns: &indexmap::IndexMap<String, String>,
) -> bool {
    let mut matches = 0_usize;
    let mut checked = 0_usize;
    for ((source_name, target_name), value) in columns.iter().zip(line.split(delimiter as char)) {
        if checked >= 16 {
            break;
        }
        checked += 1;
        let value = normalize_lookup_name(value);
        if value == normalize_lookup_name(source_name)
            || value == normalize_lookup_name(target_name)
        {
            matches += 1;
        }
    }
    checked > 0 && matches * 2 >= checked
}

fn parse_csv_utf8<F>(
    path: &Path,
    delimiter: u8,
    filenum: i32,
    table: &str,
    columns: &indexmap::IndexMap<String, String>,
    projection: Option<&HashSet<String>>,
    split_dn: bool,
    handler: &mut F,
) -> Result<usize>
where
    F: FnMut(&str, Row) -> Result<()>,
{
    let mut rows = 0_usize;
    if filenum == -1 {
        let field_pairs = projected_position_fields(columns, projection);
        let reader = BufReader::new(File::open(path)?);
        for (line_idx, line) in reader.lines().enumerate() {
            let line = line?;
            if line_idx == 0 && looks_like_header_line(&line, delimiter, columns) {
                continue;
            }
            let mut row = Row::with_capacity(field_pairs.len() + 4);
            fill_projected_position_row(&mut row, &line, delimiter, &field_pairs);
            enrich_row(&mut row, path, &HashMap::new(), split_dn);
            handler(table, row)?;
            rows += 1;
        }
    } else {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(true)
            .flexible(true)
            .from_reader(BufReader::new(File::open(path)?));

        let header_record = reader.headers()?.clone();
        let headers: Vec<String> = header_record.iter().map(normalize_lookup_name).collect();
        let column_indexes = build_column_indexes(&headers, columns, projection);
        let is_nsa_idx = headers.iter().position(|header| header == "IS_NSA");
        let dn_idx = headers.iter().position(|header| header == "DN");

        for record in reader.records() {
            let record = record?;
            let mut row = Row::with_capacity(column_indexes.len() + 4);
            for (target_name, idx) in &column_indexes {
                let value = idx
                    .and_then(|idx| record.get(idx))
                    .map(normalize_value)
                    .unwrap_or_default();
                row.insert(target_name.clone(), value);
            }
            let source = small_enrich_source(&record, is_nsa_idx, dn_idx);
            enrich_row(&mut row, path, &source, split_dn);
            handler(table, row)?;
            rows += 1;
        }
    }
    Ok(rows)
}

fn parse_csv_text<F>(
    path: &Path,
    content: &str,
    delimiter: u8,
    filenum: i32,
    table: &str,
    columns: &indexmap::IndexMap<String, String>,
    projection: Option<&HashSet<String>>,
    split_dn: bool,
    handler: &mut F,
) -> Result<usize>
where
    F: FnMut(&str, Row) -> Result<()>,
{
    let mut rows = 0_usize;
    if filenum == -1 {
        let field_pairs = projected_position_fields(columns, projection);
        for (line_idx, line) in content.lines().enumerate() {
            if line_idx == 0 && looks_like_header_line(line, delimiter, columns) {
                continue;
            }
            let mut row = Row::with_capacity(field_pairs.len() + 4);
            fill_projected_position_row(&mut row, line, delimiter, &field_pairs);
            enrich_row(&mut row, path, &HashMap::new(), split_dn);
            handler(table, row)?;
            rows += 1;
        }
    } else {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(true)
            .flexible(true)
            .from_reader(content.as_bytes());

        let header_record = reader.headers()?.clone();
        let headers: Vec<String> = header_record.iter().map(normalize_lookup_name).collect();
        let column_indexes = build_column_indexes(&headers, columns, projection);
        let is_nsa_idx = headers.iter().position(|header| header == "IS_NSA");
        let dn_idx = headers.iter().position(|header| header == "DN");

        for record in reader.records() {
            let record = record?;
            let mut row = Row::with_capacity(column_indexes.len() + 4);
            for (target_name, idx) in &column_indexes {
                let value = idx
                    .and_then(|idx| record.get(idx))
                    .map(normalize_value)
                    .unwrap_or_default();
                row.insert(target_name.clone(), value);
            }
            let source = small_enrich_source(&record, is_nsa_idx, dn_idx);
            enrich_row(&mut row, path, &source, split_dn);
            handler(table, row)?;
            rows += 1;
        }
    }
    Ok(rows)
}

fn is_utf8_encoding(encoding: &str) -> bool {
    encoding.eq_ignore_ascii_case("UTF-8") || encoding.eq_ignore_ascii_case("UTF8")
}

fn build_column_indexes(
    headers: &[String],
    columns: &indexmap::IndexMap<String, String>,
    projection: Option<&HashSet<String>>,
) -> Vec<(String, Option<usize>)> {
    columns
        .iter()
        .filter(|(_, target_name)| should_include_field(target_name, projection))
        .map(|(source_name, target_name)| {
            let idx = source_candidates(source_name, target_name)
                .into_iter()
                .find_map(|candidate| headers.iter().position(|header| header == &candidate));
            (target_name.clone(), idx)
        })
        .collect()
}

fn projected_position_fields(
    columns: &indexmap::IndexMap<String, String>,
    projection: Option<&HashSet<String>>,
) -> Vec<(usize, String)> {
    columns
        .values()
        .enumerate()
        .filter(|(_, target_name)| should_include_field(target_name, projection))
        .map(|(idx, target_name)| (idx, target_name.clone()))
        .collect()
}

fn projected_schema_position_fields(
    columns: &indexmap::IndexMap<String, String>,
    schema: &[String],
) -> Vec<(usize, usize)> {
    columns
        .values()
        .enumerate()
        .filter_map(|(source_idx, target_name)| {
            schema
                .iter()
                .position(|field| field.eq_ignore_ascii_case(target_name))
                .map(|schema_idx| (source_idx, schema_idx))
        })
        .collect()
}

fn projected_schema_header_fields(
    headers: &[String],
    columns: &indexmap::IndexMap<String, String>,
    schema: &[String],
) -> Vec<(usize, usize)> {
    columns
        .iter()
        .filter_map(|(source_name, target_name)| {
            let source_idx = source_candidates(source_name, target_name)
                .into_iter()
                .find_map(|candidate| headers.iter().position(|header| header == &candidate))?;
            let schema_idx = schema
                .iter()
                .position(|field| field.eq_ignore_ascii_case(target_name))?;
            Some((source_idx, schema_idx))
        })
        .collect()
}

fn fill_projected_position_values(
    line: &str,
    delimiter: u8,
    fields: &[(usize, usize)],
    len: usize,
) -> Vec<String> {
    let mut values = vec![String::new(); len];
    let bytes = line.as_bytes();
    let mut column_idx = 0_usize;
    let mut start = 0_usize;
    for (target_idx, schema_idx) in fields {
        while column_idx < *target_idx && start <= bytes.len() {
            match bytes[start..].iter().position(|byte| *byte == delimiter) {
                Some(offset) => start += offset + 1,
                None => start = bytes.len() + 1,
            }
            column_idx += 1;
        }

        if column_idx == *target_idx && start <= bytes.len() {
            let end = bytes[start..]
                .iter()
                .position(|byte| *byte == delimiter)
                .map(|offset| start + offset)
                .unwrap_or(bytes.len());
            values[*schema_idx] = normalize_value(&line[start..end]);
        }
    }
    values
}

fn normalize_projection_fields(fields: &HashSet<String>) -> HashSet<String> {
    fields
        .iter()
        .map(|field| normalize_lookup_name(field))
        .collect()
}

fn fill_projected_position_row(
    row: &mut Row,
    line: &str,
    delimiter: u8,
    fields: &[(usize, String)],
) {
    let bytes = line.as_bytes();
    let mut column_idx = 0_usize;
    let mut start = 0_usize;
    for (target_idx, target_field) in fields {
        while column_idx < *target_idx && start <= bytes.len() {
            match bytes[start..].iter().position(|byte| *byte == delimiter) {
                Some(offset) => start += offset + 1,
                None => start = bytes.len() + 1,
            }
            column_idx += 1;
        }

        if column_idx == *target_idx && start <= bytes.len() {
            let end = bytes[start..]
                .iter()
                .position(|byte| *byte == delimiter)
                .map(|offset| start + offset)
                .unwrap_or(bytes.len());
            row.insert(target_field.clone(), normalize_value(&line[start..end]));
        } else {
            row.insert(target_field.clone(), String::new());
        }
    }
}

fn should_include_field(field: &str, projection: Option<&HashSet<String>>) -> bool {
    let Some(projection) = projection else {
        return true;
    };
    projection.contains(&normalize_lookup_name(field))
}

fn should_split_dn(projection: Option<&HashSet<String>>) -> bool {
    let Some(projection) = projection else {
        return true;
    };
    projection.iter().any(|field| {
        matches!(
            field.as_str(),
            "MANAGEDELEMENT"
                | "GNBDUFUNCTION"
                | "GNBCUCPFUNCTION"
                | "ENBFUNCTION"
                | "NRCELLDU"
                | "NRCELLCU"
                | "EUTRANCELL"
                | "PARENT_DN"
        )
    })
}

fn source_candidates(source_name: &str, target_name: &str) -> Vec<String> {
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

    let mut seen = std::collections::HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

fn small_enrich_source(
    record: &csv::StringRecord,
    is_nsa_idx: Option<usize>,
    dn_idx: Option<usize>,
) -> HashMap<String, String> {
    let mut source = HashMap::new();
    if let Some(value) = is_nsa_idx.and_then(|idx| record.get(idx)) {
        source.insert("IS_NSA".to_string(), normalize_value(value));
    }
    if let Some(value) = dn_idx.and_then(|idx| record.get(idx)) {
        source.insert("DN".to_string(), normalize_value(value));
    }
    source
}

fn enrich_row(
    row: &mut Row,
    path: &Path,
    source: &HashMap<String, String>,
    split_dn_enabled: bool,
) {
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
    if split_dn_enabled && !dn.is_empty() {
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
        let normalized_key = normalize_lookup_name(key).to_ascii_lowercase();
        row.entry(normalized_key)
            .or_insert_with(|| value.to_string());
    }
    if let Some(idx) = dn.rfind(',') {
        row.entry("parent_dn".to_string())
            .or_insert_with(|| dn[..idx].to_string());
    }
}
