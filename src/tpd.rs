use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Result};
use chrono::{Local, NaiveDateTime};
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
    pub enabled: bool,
    pub source_table: String,
    #[serde(default)]
    pub where_expr: String,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub join_keys: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct FieldRule {
    pub name: String,
    #[serde(default)]
    pub expression: String,
    #[serde(default)]
    pub related_group: String,
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
    let source_rows = match tables
        .get(&source_key)
        .or_else(|| tables.get(&group.source_table))
    {
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
            .map(|field| eval_group_by_expr(row, field))
            .collect::<Vec<_>>()
            .join("\u{1f}");
        grouped.entry(key).or_default().push(row);
    }

    let mut output_rows = Vec::new();
    let group_elapsed = t.elapsed();
    let temp_start = Instant::now();
    let mut temp_elapsed = std::time::Duration::ZERO;
    let mut output_elapsed = std::time::Duration::ZERO;
    for rows in grouped.values() {
        let mut context = Row::new();
        for field in &group.group_by {
            context.insert(field.clone(), eval_group_by_expr(rows[0], field));
        }

        let temp_row_start = Instant::now();
        for field in &rule.temp_fields {
            let value = eval_expression(&field.expression, rows, &context, None);
            context.insert(field.name.trim().to_string(), value);
        }
        temp_elapsed += temp_row_start.elapsed();

        let output_row_start = Instant::now();
        let mut output = Row::new();
        for field in &rule.output_fields {
            let value = eval_expression(&field.expression, rows, &context, Some(&output));
            output.insert(field.name.trim().to_string(), value);
        }
        output_elapsed += output_row_start.elapsed();
        output_rows.push(output);
    }

    eprintln!(
        "  -> {} groups -> {} output rows (group={:.2}s temp={:.2}s output={:.2}s overhead={:.2}s total={:.2}s)",
        grouped.len(),
        output_rows.len(),
        group_elapsed.as_secs_f64(),
        temp_elapsed.as_secs_f64(),
        output_elapsed.as_secs_f64(),
        temp_start.elapsed().saturating_sub(temp_elapsed + output_elapsed).as_secs_f64(),
        t.elapsed().as_secs_f64(),
    );
    tables.insert(rule.table_name.to_ascii_uppercase(), output_rows);
    Ok(())
}

fn eval_expression(expr: &str, rows: &[&Row], context: &Row, output: Option<&Row>) -> String {
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
        return max_value(rows, inner, context, output);
    }
    if lower.starts_with("substring(") && expr.ends_with(')') {
        return eval_string_expr(expr, rows[0], context, output);
    }
    if lower.starts_with("timestamp14(") && expr.ends_with(')') {
        return eval_string_expr(expr, rows[0], context, output);
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

        let value = eval_expression(inner, rows, context, output);
        return crate::crc64::crc64_ecma(&value).to_string();
    }
    if lower.starts_with("case when ") {
        return eval_case_when(expr, rows, context, output);
    }
    if expr.contains("||") {
        return expr
            .split("||")
            .map(|part| eval_concat_part(part, rows, context, output))
            .collect::<Vec<_>>()
            .join("");
    }
    if let Some(value) = parse_quoted_literal(expr) {
        return value;
    }
    if let Some(value) = get_eval_context_value(context, output, expr) {
        return value;
    }
    get_row_value(rows[0], expr)
}

fn eval_concat_part(part: &str, rows: &[&Row], context: &Row, output: Option<&Row>) -> String {
    let part = part.trim();
    if let Some(value) = parse_quoted_literal(part) {
        return value;
    }
    if let Some(value) = parse_quoted_env(part) {
        return value;
    }
    if let Some(value) = get_eval_context_value(context, output, part) {
        return value;
    }
    get_row_value(rows[0], part)
}

fn eval_group_by_expr(row: &Row, expr: &str) -> String {
    let rows = [row];
    eval_expression(expr, &rows, &Row::new(), None)
}

fn eval_case_when(expr: &str, rows: &[&Row], context: &Row, output: Option<&Row>) -> String {
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
    if eval_condition(condition, context, output) {
        eval_expression(then_expr, rows, context, output)
    } else {
        eval_expression(else_expr, rows, context, output)
    }
}

fn eval_condition(condition: &str, context: &Row, output: Option<&Row>) -> bool {
    if let Some((left, right)) = condition.split_once('>') {
        let left_value = get_eval_context_value(context, output, left.trim()).unwrap_or_default();
        let right_value = right.trim().parse::<f64>().unwrap_or(0.0);
        return left_value.parse::<f64>().unwrap_or(0.0) > right_value;
    }
    false
}

fn max_value(rows: &[&Row], expr: &str, context: &Row, output: Option<&Row>) -> String {
    let simple_field = is_simple_field_expr(expr);
    if rows.len() == 1 {
        return if simple_field {
            get_row_value(rows[0], expr)
        } else {
            eval_string_expr(expr, rows[0], context, output)
        };
    }

    let mut best = String::new();
    let mut best_number: Option<f64> = None;
    for row in rows {
        let value = if simple_field {
            get_row_value(row, expr)
        } else {
            eval_string_expr(expr, row, context, output)
        };
        if value.is_empty() {
            continue;
        }
        if let Ok(number) = value.parse::<f64>() {
            if best_number.is_none_or(|best| number > best) {
                best_number = Some(number);
                best = value;
            }
        } else if best_number.is_none() && value > best {
            best = value;
        }
    }
    best
}

fn is_simple_field_expr(expr: &str) -> bool {
    !expr.contains('(')
        && !expr.contains(')')
        && !expr.contains('+')
        && !expr.contains('-')
        && !expr.contains(',')
        && !expr.contains('"')
        && !expr.contains('\'')
        && !expr.contains("||")
}

fn eval_string_expr(expr: &str, row: &Row, context: &Row, output: Option<&Row>) -> String {
    let expr = expr.trim();
    if let Some(value) = parse_quoted_literal(expr) {
        return value;
    }
    if expr.to_ascii_lowercase().starts_with("substring(") && expr.ends_with(')') {
        let inner = &expr[10..expr.len() - 1];
        let args = split_args(inner);
        if args.len() != 3 {
            return String::new();
        }
        let value = eval_string_expr(&args[0], row, context, output);
        let start = eval_number_expr(&args[1], row, context, output);
        let len = eval_number_expr(&args[2], row, context, output);
        return sql_substring(&value, start, len);
    }
    if expr.to_ascii_lowercase().starts_with("timestamp14(") && expr.ends_with(')') {
        let inner = &expr[12..expr.len() - 1];
        let value = eval_string_expr(inner, row, context, output);
        return extract_timestamp14(&value).unwrap_or_default();
    }
    get_eval_context_value(context, output, expr).unwrap_or_else(|| get_row_value(row, expr))
}

fn eval_number_expr(expr: &str, row: &Row, context: &Row, output: Option<&Row>) -> isize {
    let expr = expr.trim();
    if let Some((left, right)) = split_top_level_operator(expr, '+') {
        return eval_number_expr(left, row, context, output)
            + eval_number_expr(right, row, context, output);
    }
    if let Some((left, right)) = split_top_level_operator(expr, '-') {
        return eval_number_expr(left, row, context, output)
            - eval_number_expr(right, row, context, output);
    }
    let lower = expr.to_ascii_lowercase();
    if lower.starts_with("locate(") && expr.ends_with(')') {
        let inner = &expr[7..expr.len() - 1];
        let args = split_args(inner);
        if args.len() != 2 {
            return 0;
        }
        let needle = eval_string_expr(&args[0], row, context, output);
        let haystack = eval_string_expr(&args[1], row, context, output);
        return sql_locate(&needle, &haystack);
    }
    if lower.starts_with("length(") && expr.ends_with(')') {
        let inner = &expr[7..expr.len() - 1];
        return eval_string_expr(inner, row, context, output)
            .chars()
            .count() as isize;
    }
    if let Ok(value) = expr.parse::<isize>() {
        return value;
    }
    eval_string_expr(expr, row, context, output)
        .parse::<isize>()
        .unwrap_or(0)
}

fn split_args(expr: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    let mut in_quote = false;
    for (idx, ch) in expr.char_indices() {
        match ch {
            '\'' => in_quote = !in_quote,
            '(' if !in_quote => depth += 1,
            ')' if !in_quote => depth -= 1,
            ',' if !in_quote && depth == 0 => {
                args.push(expr[start..idx].trim().to_string());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    args.push(expr[start..].trim().to_string());
    args
}

fn split_top_level_operator(expr: &str, operator: char) -> Option<(&str, &str)> {
    let mut depth = 0;
    let mut in_quote = false;
    for (idx, ch) in expr.char_indices().rev() {
        match ch {
            '\'' => in_quote = !in_quote,
            ')' if !in_quote => depth += 1,
            '(' if !in_quote => depth -= 1,
            _ if ch == operator && !in_quote && depth == 0 => {
                return Some((&expr[..idx], &expr[idx + ch.len_utf8()..]));
            }
            _ => {}
        }
    }
    None
}

fn sql_locate(needle: &str, haystack: &str) -> isize {
    haystack
        .find(needle)
        .map(|idx| haystack[..idx].chars().count() as isize + 1)
        .unwrap_or(0)
}

fn sql_substring(value: &str, start: isize, len: isize) -> String {
    if start <= 0 || len <= 0 {
        return String::new();
    }
    value
        .chars()
        .skip((start - 1) as usize)
        .take(len as usize)
        .collect()
}

fn extract_timestamp14(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    for start in 0..bytes.len().saturating_sub(13) {
        let end = start + 14;
        if !bytes[start..end].iter().all(u8::is_ascii_digit) {
            continue;
        }
        if start > 0 && bytes[start - 1].is_ascii_digit() {
            continue;
        }
        if end < bytes.len() && bytes[end].is_ascii_digit() {
            continue;
        }
        let Ok(candidate) = std::str::from_utf8(&bytes[start..end]) else {
            continue;
        };
        if let Ok(parsed) = NaiveDateTime::parse_from_str(candidate, "%Y%m%d%H%M%S") {
            return Some(parsed.format("%Y-%m-%d %H:%M:%S").to_string());
        }
    }
    None
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

fn get_eval_context_value(context: &Row, output: Option<&Row>, field: &str) -> Option<String> {
    output
        .and_then(|output| get_context_value(output, field))
        .or_else(|| get_context_value(context, field))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row_with_object_rdn(value: &str) -> Row {
        let mut row = Row::new();
        row.insert("object_rdn".to_string(), value.to_string());
        row
    }

    #[test]
    fn evaluates_configured_substring_object_dn() {
        let row = row_with_object_rdn(
            "8105:ZTE-CMAH-HF,SubNetwork=500,ManagedElement=1561205,EnbFunction=379834,EutranCellFdd=6",
        );
        let rows = vec![&row];
        let context = Row::new();

        let value = eval_expression(
            "max(substring(object_rdn,locate('8105:',object_rdn)+5,length(object_rdn)))",
            &rows,
            &context,
            None,
        );

        assert_eq!(
            value,
            "ZTE-CMAH-HF,SubNetwork=500,ManagedElement=1561205,EnbFunction=379834,EutranCellFdd=6"
        );
    }

    #[test]
    fn evaluates_configured_substring_parent_dn() {
        let row = row_with_object_rdn(
            "8105:ZTE-CMAH-HF,SubNetwork=500,ManagedElement=1561205,EnbFunction=379834,EutranCellFdd=6",
        );
        let rows = vec![&row];
        let context = Row::new();

        let value = eval_expression(
            "max(substring(object_rdn,locate('8105:',object_rdn)+5,locate(',EutranCell',object_rdn)-6))",
            &rows,
            &context,
            None,
        );

        assert_eq!(
            value,
            "ZTE-CMAH-HF,SubNetwork=500,ManagedElement=1561205,EnbFunction=379834"
        );
    }

    #[test]
    fn evaluates_timestamp14_from_source_filename() {
        let mut row = Row::new();
        row.insert(
            "SOURCEFILENAME".to_string(),
            "PM-ENB-EUTRANCELLTDD-2A-V3.5.0-20220110130000-15.csv.gz".to_string(),
        );
        let rows = vec![&row];
        let context = Row::new();

        let value = eval_expression("max(timestamp14(SOURCEFILENAME))", &rows, &context, None);

        assert_eq!(value, "2022-01-10 13:00:00");
    }

    #[test]
    fn timestamp14_supports_year_2030() {
        assert_eq!(
            extract_timestamp14("PM-ENB-EUTRANCELLTDD-2A-V3.5.0-20300110130000-15.csv.gz")
                .as_deref(),
            Some("2030-01-10 13:00:00")
        );
    }
}
