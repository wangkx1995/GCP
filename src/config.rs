use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use indexmap::IndexMap;

use crate::util::{file_name, read_text};

pub struct MappingConfig {
    pub table_mapping: HashMap<String, String>,
    pub columns: IndexMap<String, IndexMap<String, String>>,
    pub filenum: i32,
}

pub struct ContextData {
    pub mapping: MappingConfig,
    pub encoding: String,
}

pub fn parse_mapping_config(path: &Path) -> Result<MappingConfig> {
    let text = read_text(path, "UTF-8")?;
    let mut section = String::new();
    let mut table_mapping = HashMap::new();
    let mut columns: IndexMap<String, IndexMap<String, String>> = IndexMap::new();
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
                    if parts[0].eq_ignore_ascii_case("filenum") {
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
                }
            }
            _ => {}
        }
    }

    Ok(MappingConfig {
        table_mapping,
        columns,
        filenum,
    })
}

fn split_mapping_line(line: &str) -> Vec<&str> {
    line.split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

pub fn resolve_table(mapping: &MappingConfig, counter: &str, path: &Path) -> Result<String> {
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

pub fn detect_counter_from_filename(path: &Path, mapping: &MappingConfig) -> Result<String> {
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
