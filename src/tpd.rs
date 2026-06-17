use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use chrono::{Local, NaiveDateTime};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer};

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
    #[serde(deserialize_with = "deserialize_source_tables")]
    pub source_table: Vec<String>,
    #[serde(default)]
    pub where_expr: String,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub join_keys: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SourceTables {
    One(String),
    Many(Vec<String>),
}

fn deserialize_source_tables<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let source_tables = SourceTables::deserialize(deserializer)?;
    let values = match source_tables {
        SourceTables::One(value) => vec![value],
        SourceTables::Many(values) => values,
    };
    Ok(values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect())
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
    let mut source_rows = Vec::new();
    for source_table in &group.source_table {
        let source_key = source_table.to_ascii_uppercase();
        match tables.get(&source_key).or_else(|| tables.get(source_table)) {
            Some(rows) => source_rows.extend(rows.iter()),
            None => eprintln!(
                "[aggregate] WARN {} <- {}: source table not found, skipping",
                rule.table_name, source_table,
            ),
        }
    }
    if source_rows.is_empty() {
        let available: Vec<&String> = tables.keys().collect();
        eprintln!(
            "[aggregate] SKIP {} <- {:?}: source table not found. Available tables: {:?}",
            rule.table_name, group.source_table, available,
        );
        return Ok(());
    }

    eprintln!(
        "[aggregate] {} <- {:?} ({} source rows, group by {:?})",
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
            .map(|field| {
                eval_group_by_expr(row, field).with_context(|| {
                    format!(
                        "rule {} group {} group_by expression {} failed",
                        rule.table_name, group.name, field
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?
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
            let value = eval_group_by_expr(rows[0], field).with_context(|| {
                format!(
                    "rule {} group {} group_by expression {} failed",
                    rule.table_name, group.name, field
                )
            })?;
            context.insert(field.clone(), value);
        }

        let temp_row_start = Instant::now();
        for field in &rule.temp_fields {
            let value =
                eval_expression(&field.expression, rows, &context, None).with_context(|| {
                    format!(
                        "rule {} temp field {} expression {} failed",
                        rule.table_name, field.name, field.expression
                    )
                })?;
            context.insert(field.name.trim().to_string(), value);
        }
        temp_elapsed += temp_row_start.elapsed();

        let output_row_start = Instant::now();
        let mut output = Row::new();
        for field in &rule.output_fields {
            let value = eval_expression(&field.expression, rows, &context, Some(&output))
                .with_context(|| {
                    format!(
                        "rule {} output field {} expression {} failed",
                        rule.table_name, field.name, field.expression
                    )
                })?;
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

fn eval_expression(
    expr: &str,
    rows: &[&Row],
    context: &Row,
    output: Option<&Row>,
) -> Result<String> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Ok(String::new());
    }
    if expr.parse::<f64>().is_ok() {
        return Ok(expr.to_string());
    }
    let lower = expr.to_ascii_lowercase();

    if lower == "null" {
        return Ok(String::new());
    }
    if lower == "current_timestamp" {
        return Ok(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
    }
    if let Some(value) = parse_quoted_env(expr) {
        return Ok(value);
    }
    if lower.starts_with("max(") && expr.ends_with(')') {
        let inner = &expr[4..expr.len() - 1];
        return max_value(rows, inner, context, output);
    }
    if lower.starts_with("lower(") && expr.ends_with(')') {
        let inner = &expr[6..expr.len() - 1];
        return Ok(eval_expression(inner, rows, context, output)?.to_ascii_lowercase());
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
            values.insert(require_row_value(row, inner)?);
        }
        return Ok(values.len().to_string());
    }
    if lower.starts_with("crc64(") && expr.ends_with(')') {
        let inner = &expr[6..expr.len() - 1];

        let value = eval_expression(inner, rows, context, output)?;
        return Ok(crate::crc64::crc64_ecma(&value).to_string());
    }
    if lower.starts_with("case when ") {
        return eval_case_when(expr, rows, context, output);
    }
    if expr.contains("||") {
        return Ok(expr
            .split("||")
            .map(|part| eval_concat_part(part, rows, context, output))
            .collect::<Result<Vec<_>>>()?
            .join(""));
    }
    if let Some(value) = parse_quoted_literal(expr) {
        return Ok(value);
    }
    if let Some(value) = get_eval_context_value(context, output, expr) {
        return Ok(value);
    }
    require_row_value(rows[0], expr)
}

fn eval_concat_part(
    part: &str,
    rows: &[&Row],
    context: &Row,
    output: Option<&Row>,
) -> Result<String> {
    let part = part.trim();
    if let Some(value) = parse_quoted_literal(part) {
        return Ok(value);
    }
    if let Some(value) = parse_quoted_env(part) {
        return Ok(value);
    }
    let lower = part.to_ascii_lowercase();
    if lower.starts_with("max(")
        || lower.starts_with("lower(")
        || lower.starts_with("substring(")
        || lower.starts_with("timestamp14(")
        || lower.starts_with("crc64(")
        || lower.starts_with("case when ")
    {
        return eval_expression(part, rows, context, output);
    }
    if let Some(value) = get_eval_context_value(context, output, part) {
        return Ok(value);
    }
    require_row_value(rows[0], part)
}

fn eval_group_by_expr(row: &Row, expr: &str) -> Result<String> {
    let rows = [row];
    eval_expression(expr, &rows, &Row::new(), None)
}

fn eval_case_when(
    expr: &str,
    rows: &[&Row],
    context: &Row,
    output: Option<&Row>,
) -> Result<String> {
    let Some(mut rest) = strip_case_expr(expr) else {
        return Ok(String::new());
    };

    loop {
        let rest_lower = rest.to_ascii_lowercase();
        if rest_lower.starts_with("when ") {
            let Some(then_idx) = find_case_keyword(rest, " then ") else {
                return Ok(String::new());
            };
            let condition = rest[5..then_idx].trim();
            let after_then = &rest[then_idx + 6..];
            let next_when = find_case_keyword(after_then, " when ");
            let next_else = find_case_keyword(after_then, " else ");
            let end_idx = match (next_when, next_else) {
                (Some(when_idx), Some(else_idx)) => when_idx.min(else_idx),
                (Some(when_idx), None) => when_idx,
                (None, Some(else_idx)) => else_idx,
                (None, None) => after_then.len(),
            };
            let then_expr = after_then[..end_idx].trim();
            if eval_condition(condition, rows, context, output)? {
                return eval_expression(then_expr, rows, context, output);
            }
            rest = after_then[end_idx..].trim();
            continue;
        }
        if rest_lower.starts_with("else ") {
            return eval_expression(rest[5..].trim(), rows, context, output);
        }
        return Ok(String::new());
    }
}

fn strip_case_expr(expr: &str) -> Option<&str> {
    let trimmed = expr.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("case ") || !lower.ends_with(" end") {
        return None;
    }
    Some(trimmed[5..trimmed.len() - 4].trim())
}

fn find_case_keyword(expr: &str, keyword: &str) -> Option<usize> {
    let lower = expr.to_ascii_lowercase();
    lower.find(keyword)
}

fn eval_condition(
    condition: &str,
    rows: &[&Row],
    context: &Row,
    output: Option<&Row>,
) -> Result<bool> {
    if let Some((left, right)) = condition.split_once('=') {
        let left_value = eval_condition_operand(left.trim(), rows, context, output)?;
        let right_value = eval_condition_operand(right.trim(), rows, context, output)?;
        return Ok(left_value == right_value);
    }
    if let Some((left, right)) = condition.split_once('>') {
        let left_value = eval_condition_operand(left.trim(), rows, context, output)?;
        let right_value = right.trim().parse::<f64>().unwrap_or(0.0);
        return Ok(left_value.parse::<f64>().unwrap_or(0.0) > right_value);
    }
    Ok(false)
}

fn eval_condition_operand(
    expr: &str,
    rows: &[&Row],
    context: &Row,
    output: Option<&Row>,
) -> Result<String> {
    let expr = expr.trim();
    if expr.eq_ignore_ascii_case("null") {
        return Ok(String::new());
    }
    if let Some(value) = parse_quoted_literal(expr) {
        return Ok(value);
    }
    if expr.parse::<f64>().is_ok() {
        return Ok(expr.to_string());
    }
    if let Some(value) = get_eval_context_value(context, output, expr) {
        return Ok(value);
    }
    require_row_value(rows[0], expr)
}

fn max_value(rows: &[&Row], expr: &str, context: &Row, output: Option<&Row>) -> Result<String> {
    let simple_field = is_simple_field_expr(expr);
    if rows.len() == 1 {
        return Ok(if simple_field {
            require_row_value(rows[0], expr)?
        } else {
            eval_string_expr(expr, rows[0], context, output)?
        });
    }

    let mut best = String::new();
    let mut best_number: Option<f64> = None;
    for row in rows {
        let value = if simple_field {
            require_row_value(row, expr)?
        } else {
            eval_string_expr(expr, row, context, output)?
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
    Ok(best)
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

fn eval_string_expr(expr: &str, row: &Row, context: &Row, output: Option<&Row>) -> Result<String> {
    let expr = expr.trim();
    if let Some(value) = parse_quoted_literal(expr) {
        return Ok(value);
    }
    if expr.to_ascii_lowercase().starts_with("substring(") && expr.ends_with(')') {
        let inner = &expr[10..expr.len() - 1];
        let args = split_args(inner);
        if args.len() != 3 {
            return Ok(String::new());
        }
        let value = eval_string_expr(&args[0], row, context, output)?;
        let start = eval_number_expr(&args[1], row, context, output)?;
        let len = eval_number_expr(&args[2], row, context, output)?;
        return Ok(sql_substring(&value, start, len));
    }
    if expr.to_ascii_lowercase().starts_with("timestamp14(") && expr.ends_with(')') {
        let inner = &expr[12..expr.len() - 1];
        let value = eval_string_expr(inner, row, context, output)?;
        return Ok(extract_timestamp14(&value).unwrap_or_default());
    }
    if let Some(value) = get_eval_context_value(context, output, expr) {
        return Ok(value);
    }
    require_row_value(row, expr)
}

fn eval_number_expr(expr: &str, row: &Row, context: &Row, output: Option<&Row>) -> Result<isize> {
    let expr = expr.trim();
    if let Some((left, right)) = split_top_level_operator(expr, '+') {
        return Ok(eval_number_expr(left, row, context, output)?
            + eval_number_expr(right, row, context, output)?);
    }
    if let Some((left, right)) = split_top_level_operator(expr, '-') {
        return Ok(eval_number_expr(left, row, context, output)?
            - eval_number_expr(right, row, context, output)?);
    }
    let lower = expr.to_ascii_lowercase();
    if lower.starts_with("locate(") && expr.ends_with(')') {
        let inner = &expr[7..expr.len() - 1];
        let args = split_args(inner);
        if args.len() != 2 {
            return Ok(0);
        }
        let needle = eval_string_expr(&args[0], row, context, output)?;
        let haystack = eval_string_expr(&args[1], row, context, output)?;
        return Ok(sql_locate(&needle, &haystack));
    }
    if lower.starts_with("length(") && expr.ends_with(')') {
        let inner = &expr[7..expr.len() - 1];
        return Ok(eval_string_expr(inner, row, context, output)?
            .chars()
            .count() as isize);
    }
    if let Ok(value) = expr.parse::<isize>() {
        return Ok(value);
    }
    Ok(eval_string_expr(expr, row, context, output)?
        .parse::<isize>()
        .unwrap_or(0))
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

fn require_row_value(row: &Row, field: &str) -> Result<String> {
    find_row_value(row, field).with_context(|| format!("missing field {}", field))
}

fn find_row_value(row: &Row, field: &str) -> Option<String> {
    if let Some(value) = row.get(field) {
        return Some(value.clone());
    }
    let normalized = normalize_lookup_name(field);
    for (key, value) in row {
        if normalize_lookup_name(key) == normalized {
            return Some(value.clone());
        }
    }
    None
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
        )
        .unwrap();

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
        )
        .unwrap();

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

        let value =
            eval_expression("max(timestamp14(SOURCEFILENAME))", &rows, &context, None).unwrap();

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

    #[test]
    fn missing_output_field_reference_errors() {
        let row = row_with_object_rdn(
            "8105:ZTE-CMAH-HF,SubNetwork=500,ManagedElement=1561205,EnbFunction=379834,EutranCellFdd=6",
        );
        let rows = vec![&row];
        let context = Row::new();

        let err = eval_expression("CRC64('8104:'||parent_dn)", &rows, &context, None).unwrap_err();

        assert!(err.to_string().contains("missing field parent_dn"));
    }

    #[test]
    fn existing_empty_field_is_allowed() {
        let mut row = Row::new();
        row.insert("parent_dn".to_string(), String::new());
        let rows = vec![&row];
        let context = Row::new();

        let value = eval_expression("CRC64('8104:'||parent_dn)", &rows, &context, None).unwrap();

        assert!(!value.is_empty());
    }

    #[test]
    fn parses_string_source_table() {
        let rule: TpdRule = serde_json::from_str(
            r#"{
              "table_name": "TPD_TEST",
              "groups": [{"name":"related_rdn01","enabled":true,"source_table":"OP_A"}],
              "temp_fields": [],
              "output_fields": []
            }"#,
        )
        .unwrap();

        assert_eq!(rule.groups[0].source_table, vec!["OP_A"]);
    }

    #[test]
    fn parses_array_source_table() {
        let rule: TpdRule = serde_json::from_str(
            r#"{
              "table_name": "TPD_TEST",
              "groups": [{"name":"related_rdn01","enabled":true,"source_table":["OP_A","OP_B"]}],
              "temp_fields": [],
              "output_fields": []
            }"#,
        )
        .unwrap();

        assert_eq!(rule.groups[0].source_table, vec!["OP_A", "OP_B"]);
    }

    #[test]
    fn evaluates_lower_max_expression() {
        let mut row = Row::new();
        row.insert("VENDORNAME".to_string(), "ZTE".to_string());
        let rows = vec![&row];
        let context = Row::new();

        let value = eval_expression("lower(max(VENDORNAME))", &rows, &context, None).unwrap();

        assert_eq!(value, "zte");
    }

    #[test]
    fn evaluates_multi_when_case_expression() {
        let row = Row::new();
        let rows = vec![&row];
        let mut context = Row::new();
        context.insert("vendor_id_0".to_string(), "zte".to_string());

        let value = eval_expression(
            "case when vendor_id_0='ericsson' then 1 when vendor_id_0='huawei' then 8 when vendor_id_0='nokia' then 4 when vendor_id_0='zte' then 7 else null end",
            &rows,
            &context,
            None,
        )
        .unwrap();

        assert_eq!(value, "7");
    }

    #[test]
    fn evaluates_case_else_null_as_empty() {
        let row = Row::new();
        let rows = vec![&row];
        let mut context = Row::new();
        context.insert("vendor_id_0".to_string(), "unknown".to_string());

        let value = eval_expression(
            "case when vendor_id_0='zte' then 7 else null end",
            &rows,
            &context,
            None,
        )
        .unwrap();

        assert_eq!(value, "");
    }

    #[test]
    fn missing_case_condition_field_errors() {
        let row = Row::new();
        let rows = vec![&row];
        let context = Row::new();

        let err = eval_expression(
            "case when vendor_id_0='zte' then 7 else null end",
            &rows,
            &context,
            None,
        )
        .unwrap_err();

        assert!(err.to_string().contains("missing field vendor_id_0"));
    }

    #[test]
    fn combines_available_source_tables_and_skips_missing_sources() {
        let rule: TpdRule = serde_json::from_str(
            r#"{
              "table_name": "TPD_TEST",
              "groups": [{
                "name":"related_rdn01",
                "enabled":true,
                "source_table":["OP_A","OP_MISSING"],
                "group_by":["dn"]
              }],
              "temp_fields": [],
              "output_fields": [
                {"name":"dn","expression":"max(dn)"},
                {"name":"value","expression":"max(value)"}
              ]
            }"#,
        )
        .unwrap();
        let mut row = Row::new();
        row.insert("dn".to_string(), "cell-1".to_string());
        row.insert("value".to_string(), "7".to_string());
        let mut tables = TableRows::new();
        tables.insert("OP_A".to_string(), vec![row]);

        execute_tpd_rule(&rule, &mut tables).unwrap();

        let output = tables.get("TPD_TEST").unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].get("value").map(String::as_str), Some("7"));
    }
}
