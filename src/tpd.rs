use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Result};
use chrono::Local;
use indexmap::IndexMap;
use serde::Deserialize;

use crate::util::*;
use crate::{Row, TableRows};

#[derive(Debug, Deserialize)]
pub struct TpdRule {
    pub table_name: String,
    pub groups: Vec<GroupRule>,
    pub temp_fields: Vec<FieldRule>,
    pub output_fields: Vec<FieldRule>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct GroupRule {
    pub name: String,
    #[serde(default)]
    pub group_name: String,
    #[serde(default)]
    pub enabled: bool,
    pub source_table: String,
    #[serde(default)]
    pub where_expr: String,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub order_by: Vec<String>,
    #[serde(default)]
    pub join_keys: Vec<String>,
    #[serde(default)]
    pub transform_type: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct FieldRule {
    pub name: String,
    #[serde(default)]
    pub field_cn: String,
    #[serde(default)]
    pub field_eng: String,
    #[serde(default)]
    pub data_type: String,
    #[serde(default)]
    pub constraint: String,
    #[serde(default)]
    pub default_value: String,
    #[serde(default)]
    pub expression: String,
    #[serde(default)]
    pub related_group: String,
    #[serde(default)]
    pub description: String,
}

pub fn load_rule(path: &Path) -> Result<TpdRule> {
    let text = fs::read_to_string(path)?;
    let rule = serde_json::from_str(&text)?;
    Ok(rule)
}

pub fn execute_tpd_rule(rule: &TpdRule, tables: &mut TableRows) -> Result<()> {
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