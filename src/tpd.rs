use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

use ahash::RandomState;
use anyhow::{bail, Context, Result};
use chrono::{Local, NaiveDateTime};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer};

use crate::util::*;
use crate::writer::StreamingTableWriter;
use crate::{load_config::LoadConfig, LoadType};
use crate::{Row, TableRows};
use tracing::info;

type FastHashBuilder = RandomState;
type FastHashSet<T> = HashSet<T, FastHashBuilder>;
type FastHashMap<K, V> = HashMap<K, V, FastHashBuilder>;
type FastIndexMap<K, V> = IndexMap<K, V, FastHashBuilder>;

#[derive(Clone, Debug, Deserialize)]
pub struct TpdRule {
    pub table_name: String,
    pub groups: Vec<GroupRule>,
    pub temp_fields: Vec<FieldRule>,
    pub output_fields: Vec<FieldRule>,
}

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
pub struct FieldRule {
    pub name: String,
    #[serde(default)]
    pub expression: String,
}

pub fn load_rule(path: &Path) -> Result<TpdRule> {
    let text = fs::read_to_string(path)?;
    let rule = serde_json::from_str(&text)?;
    Ok(rule)
}

pub struct StreamingTpdEngine<'a> {
    aggregators: Vec<StreamingRuleAggregator<'a>>,
}

pub struct StreamingFinishOptions<'a> {
    pub output_dir: &'a Path,
    pub delimiter: u8,
    pub collect_id: &'a str,
    pub load_type: LoadType,
    pub load_config: &'a LoadConfig,
}

impl<'a> StreamingTpdEngine<'a> {
    pub fn new(rules: &'a [TpdRule]) -> Self {
        Self {
            aggregators: build_streaming_aggregators(rules),
        }
    }

    pub fn consumes_table(&self, table: &str) -> bool {
        self.aggregators
            .iter()
            .any(|aggregator| aggregator.consumes_table(table))
    }

    pub fn accept_owned(&mut self, table: &str, row: Row) -> Result<()> {
        let matching: Vec<usize> = self
            .aggregators
            .iter()
            .enumerate()
            .filter_map(|(idx, aggregator)| aggregator.consumes_table(table).then_some(idx))
            .collect();
        if matching.len() == 1 {
            self.aggregators[matching[0]].accept_owned(row)?;
        } else {
            for idx in matching {
                self.aggregators[idx].accept(&row)?;
            }
        }
        Ok(())
    }

    pub fn accept_values(&mut self, table: &str, values: Vec<String>) -> Result<()> {
        let matching: Vec<usize> = self
            .aggregators
            .iter()
            .enumerate()
            .filter_map(|(idx, aggregator)| aggregator.consumes_table(table).then_some(idx))
            .collect();
        if matching.len() == 1 {
            self.aggregators[matching[0]].accept_values(values)?;
        } else {
            let row = row_from_values_ref(&self.aggregators[matching[0]].ordered_fields, &values);
            for idx in matching {
                self.aggregators[idx].accept(&row)?;
            }
        }
        Ok(())
    }

    pub fn finish(
        self,
        tables: &mut TableRows,
        options: &StreamingFinishOptions<'_>,
    ) -> Result<()> {
        for aggregator in self.aggregators {
            aggregator.finish(tables, options)?;
        }
        Ok(())
    }
}

fn build_streaming_aggregators<'a>(rules: &'a [TpdRule]) -> Vec<StreamingRuleAggregator<'a>> {
    let mut aggregators: Vec<StreamingRuleAggregator<'a>> = Vec::new();
    for rule in rules {
        let Some(aggregator) = StreamingRuleAggregator::new(rule) else {
            continue;
        };
        if let Some(existing) = aggregators
            .iter_mut()
            .find(|existing| existing.can_merge(&aggregator))
        {
            existing.merge(aggregator);
        } else {
            aggregators.push(aggregator);
        }
    }
    aggregators
}

pub fn validate_streaming_rules(rules: &[TpdRule]) -> Result<()> {
    let errors = rules
        .iter()
        .filter_map(|rule| {
            streaming_incompatibility_reason(rule).map(|reason| {
                format!(
                    "rule {} is not streaming-compatible: {reason}",
                    rule.table_name
                )
            })
        })
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        bail!(errors.join("; "));
    }
    Ok(())
}

fn streaming_incompatibility_reason(rule: &TpdRule) -> Option<&'static str> {
    if rule.groups.iter().filter(|group| group.enabled).count() != 1 {
        return Some("expected exactly one enabled group");
    }
    if StreamingRuleAggregator::new(rule).is_none() {
        return Some("failed to build streaming aggregator");
    }
    None
}

pub fn streaming_required_fields_by_table(rules: &[TpdRule]) -> HashMap<String, HashSet<String>> {
    let mut result = HashMap::new();
    for aggregator in build_streaming_aggregators(rules) {
        for source_table in &aggregator.source_tables {
            let fields = result
                .entry(source_table.clone())
                .or_insert_with(HashSet::new);
            fields.extend(aggregator.required_fields.iter().cloned());
        }
    }
    result
}

pub fn streaming_ordered_fields_by_table(rules: &[TpdRule]) -> HashMap<String, Vec<String>> {
    let mut result = HashMap::new();
    for aggregator in build_streaming_aggregators(rules) {
        if !matches!(
            aggregator.key_builder,
            GroupKeyBuilder::DnScanStartStop
                | GroupKeyBuilder::DnTimestamp14Source
                | GroupKeyBuilder::RdnTimestamp14Source
                | GroupKeyBuilder::ObjectRdnScanStartStop
        ) {
            continue;
        }
        let ordered = aggregator.ordered_fields.clone();
        for source_table in &aggregator.source_tables {
            let fields = result.entry(source_table.clone()).or_insert_with(Vec::new);
            for field in &ordered {
                if !fields
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(field))
                {
                    fields.push(field.clone());
                }
            }
        }
    }
    result
}

struct StreamingRuleAggregator<'a> {
    plans: Vec<StreamingRulePlan<'a>>,
    group: &'a GroupRule,
    source_tables: FastHashSet<String>,
    distinct_fields: Vec<String>,
    required_fields: FastHashSet<String>,
    ordered_fields: Vec<String>,
    field_indexes: FastHashMap<String, usize>,
    key_builder: GroupKeyBuilder,
    grouped: FastIndexMap<String, StreamingGroupState>,
}

struct StreamingRulePlan<'a> {
    rule: &'a TpdRule,
    group: &'a GroupRule,
    temp_exprs: Vec<CompiledFieldExpr<'a>>,
    output_exprs: Vec<CompiledFieldExpr<'a>>,
}

impl<'a> StreamingRulePlan<'a> {
    fn new(
        rule: &'a TpdRule,
        group: &'a GroupRule,
        field_indexes: &FastHashMap<String, usize>,
    ) -> Self {
        Self {
            rule,
            group,
            temp_exprs: rule
                .temp_fields
                .iter()
                .map(|field| CompiledFieldExpr::new(field, field_indexes))
                .collect(),
            output_exprs: rule
                .output_fields
                .iter()
                .map(|field| CompiledFieldExpr::new(field, field_indexes))
                .collect(),
        }
    }

    fn rebind_indexes(&mut self, field_indexes: &FastHashMap<String, usize>) {
        self.temp_exprs = self
            .rule
            .temp_fields
            .iter()
            .map(|field| CompiledFieldExpr::new(field, field_indexes))
            .collect();
        self.output_exprs = self
            .rule
            .output_fields
            .iter()
            .map(|field| CompiledFieldExpr::new(field, field_indexes))
            .collect();
    }
}

impl<'a> StreamingRuleAggregator<'a> {
    fn new(rule: &'a TpdRule) -> Option<Self> {
        if rule.groups.iter().filter(|group| group.enabled).count() != 1 {
            return None;
        }
        let group = rule.groups.iter().find(|group| group.enabled)?;
        let source_tables: FastHashSet<String> = group
            .source_table
            .iter()
            .map(|table| table.to_ascii_uppercase())
            .collect();
        let distinct_fields = collect_count_distinct_fields(rule);
        let mut required_fields = collect_required_source_fields(rule, group, &distinct_fields);
        if required_fields.iter().any(|field| {
            matches!(
                normalize_lookup_name(field).as_str(),
                "ENBFUNCTION"
                    | "EUTRANCELL"
                    | "MANAGEDELEMENT"
                    | "GNBDUFUNCTION"
                    | "GNBCUCPFUNCTION"
                    | "NRCELLDU"
                    | "NRCELLCU"
                    | "PARENT_DN"
            )
        }) {
            required_fields.insert("DN".to_string());
        }
        let key_builder = GroupKeyBuilder::new(&group.group_by);
        match &key_builder {
            GroupKeyBuilder::DnScanStartStop => {
                required_fields.insert("dn".to_string());
                required_fields.insert("scan_start_time".to_string());
                required_fields.insert("scan_stop_time".to_string());
                required_fields.insert("is_nsa".to_string());
            }
            GroupKeyBuilder::DnTimestamp14Source => {
                required_fields.insert("dn".to_string());
                required_fields.insert("SOURCEFILENAME".to_string());
                required_fields.insert("VENDORNAME".to_string());
                required_fields.insert("scan_start_time".to_string());
                required_fields.insert("scan_stop_time".to_string());
            }
            GroupKeyBuilder::RdnTimestamp14Source => {
                required_fields.insert("RDN".to_string());
                required_fields.insert("SOURCEFILENAME".to_string());
                required_fields.insert("VENDORNAME".to_string());
                required_fields.insert("scan_start_time".to_string());
                required_fields.insert("scan_stop_time".to_string());
            }
            GroupKeyBuilder::ObjectRdnScanStartStop => {
                required_fields.insert("object_rdn".to_string());
                required_fields.insert("scan_start_time".to_string());
                required_fields.insert("scan_stop_time".to_string());
            }
            GroupKeyBuilder::Fields(fields) => {
                required_fields.extend(fields.iter().cloned());
            }
            GroupKeyBuilder::Expressions(_) => {}
        }
        let mut ordered_fields: Vec<String> = required_fields.iter().cloned().collect();
        sort_ordered_fields(&mut ordered_fields);
        let field_indexes = build_field_indexes(&ordered_fields);
        Some(Self {
            plans: vec![StreamingRulePlan::new(rule, group, &field_indexes)],
            group,
            source_tables,
            distinct_fields,
            required_fields,
            ordered_fields,
            field_indexes,
            key_builder,
            grouped: FastIndexMap::default(),
        })
    }

    fn can_merge(&self, other: &Self) -> bool {
        self.source_tables == other.source_tables
            && self.group.where_expr == other.group.where_expr
            && self.group.group_by == other.group.group_by
    }

    fn merge(&mut self, other: Self) {
        for field in other.distinct_fields {
            if !self.distinct_fields.contains(&field) {
                self.distinct_fields.push(field);
            }
        }
        self.required_fields.extend(other.required_fields);
        for field in other.ordered_fields {
            if !self
                .ordered_fields
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&field))
            {
                self.ordered_fields.push(field);
            }
        }
        sort_ordered_fields(&mut self.ordered_fields);
        self.field_indexes = build_field_indexes(&self.ordered_fields);
        self.plans.extend(other.plans);
        self.rebind_plan_indexes();
    }

    fn rebind_plan_indexes(&mut self) {
        for plan in &mut self.plans {
            plan.rebind_indexes(&self.field_indexes);
        }
    }

    fn consumes_table(&self, table: &str) -> bool {
        self.source_tables.contains(&table.to_ascii_uppercase())
    }

    fn accept(&mut self, row: &Row) -> Result<()> {
        if !eval_where_expr(&self.group.where_expr, row)? {
            return Ok(());
        }
        let key = self.key_builder.build(row).with_context(|| {
            format!(
                "rule {} group {} group_by expression failed",
                self.plans[0].rule.table_name, self.group.name,
            )
        })?;

        match self.grouped.get_mut(&key) {
            Some(existing) => existing.update(row, &self.distinct_fields, &self.required_fields)?,
            None => {
                let mut state = StreamingGroupState::new(project_row(row, &self.required_fields)?);
                state.update_distinct(row, &self.distinct_fields)?;
                self.grouped.insert(key, state);
            }
        }
        Ok(())
    }

    fn accept_owned(&mut self, row: Row) -> Result<()> {
        if !eval_where_expr(&self.group.where_expr, &row)? {
            return Ok(());
        }
        let key = self.key_builder.build(&row).with_context(|| {
            format!(
                "rule {} group {} group_by expression failed",
                self.plans[0].rule.table_name, self.group.name,
            )
        })?;

        match self.grouped.get_mut(&key) {
            Some(existing) => {
                existing.update(&row, &self.distinct_fields, &self.required_fields)?
            }
            None => {
                let distinct = build_distinct(&row, &self.distinct_fields)?;
                let state = StreamingGroupState {
                    row,
                    values: None,
                    distinct,
                };
                self.grouped.insert(key, state);
            }
        }
        Ok(())
    }

    fn accept_values(&mut self, values: Vec<String>) -> Result<()> {
        let row;
        let row_ref = if self.group.where_expr.trim().is_empty() {
            None
        } else {
            row = row_from_values_ref(&self.ordered_fields, &values);
            if !eval_where_expr(&self.group.where_expr, &row)? {
                return Ok(());
            }
            Some(&row)
        };
        let key = self
            .key_builder
            .build_values(&self.field_indexes, &values)
            .or_else(|_| {
                let row = row_ref
                    .cloned()
                    .unwrap_or_else(|| row_from_values_ref(&self.ordered_fields, &values));
                self.key_builder.build(&row)
            })
            .with_context(|| {
                format!(
                    "rule {} group {} group_by expression failed",
                    self.plans[0].rule.table_name, self.group.name,
                )
            })?;
        match self.grouped.get_mut(&key) {
            Some(existing) => {
                merge_projected_max_values(
                    &mut existing.values,
                    &mut existing.row,
                    &self.ordered_fields,
                    &values,
                )?;
                existing.update_distinct_values(
                    &self.field_indexes,
                    &values,
                    &self.distinct_fields,
                )?;
            }
            None => {
                let distinct =
                    build_distinct_values(&self.field_indexes, &values, &self.distinct_fields)?;
                let row = row_ref
                    .cloned()
                    .unwrap_or_else(|| row_from_values_ref(&self.ordered_fields, &values));
                let state = StreamingGroupState {
                    row,
                    values: Some(values),
                    distinct,
                };
                self.grouped.insert(key, state);
            }
        }
        Ok(())
    }

    fn finish(self, _tables: &mut TableRows, options: &StreamingFinishOptions<'_>) -> Result<()> {
        if self.grouped.is_empty() {
            for plan in &self.plans {
                info!(
                    "[aggregate] SKIP {} <- {:?}: no streamed source rows",
                    plan.rule.table_name, plan.group.source_table,
                );
            }
            return Ok(());
        }

        let mut headers_by_plan = Vec::with_capacity(self.plans.len());
        let mut writers = Vec::with_capacity(self.plans.len());
        let mut starts = Vec::with_capacity(self.plans.len());
        let mut output_rows = vec![0_usize; self.plans.len()];
        for plan in &self.plans {
            let t = Instant::now();
            info!(
                "[aggregate] {} <- {:?} ({} streamed groups, group by {:?})",
                plan.rule.table_name,
                plan.group.source_table,
                self.grouped.len(),
                plan.group.group_by,
            );
            let headers = unique_output_headers(plan.rule);
            let writer = StreamingTableWriter::new_with_headers(
                headers.clone(),
                &plan.rule.table_name.to_ascii_uppercase(),
                options.output_dir,
                options.delimiter,
                options.collect_id,
                options.load_type,
                options.load_config,
            )?;
            headers_by_plan.push(headers);
            writers.push(writer);
            starts.push(t);
        }

        for state in self.grouped.values() {
            let mut temp_cache = FastHashMap::default();
            for plan_idx in 0..self.plans.len() {
                self.finish_plan_row(
                    &self.plans[plan_idx],
                    &headers_by_plan[plan_idx],
                    state,
                    &mut temp_cache,
                    &mut writers[plan_idx],
                )?;
                output_rows[plan_idx] += 1;
            }
        }

        for (plan_idx, writer) in writers.into_iter().enumerate() {
            writer.finish()?;
            info!(
                "[aggregate] {} {} streamed groups -> {} output rows ({:.2}s)",
                self.plans[plan_idx].rule.table_name,
                self.grouped.len(),
                output_rows[plan_idx],
                starts[plan_idx].elapsed().as_secs_f64(),
            );
        }
        Ok(())
    }

    fn finish_plan_row(
        &self,
        plan: &StreamingRulePlan<'_>,
        headers: &[String],
        state: &StreamingGroupState,
        temp_cache: &mut FastHashMap<String, String>,
        writer: &mut StreamingTableWriter<'_>,
    ) -> Result<()> {
        let row = &state.row;
        let rows = [row];
        let mut context = Row::new();
        for field in &plan.group.group_by {
            let value = eval_group_by_expr(row, field).with_context(|| {
                format!(
                    "rule {} group {} group_by expression {} failed",
                    plan.rule.table_name, plan.group.name, field
                )
            })?;
            context.insert(field.clone(), value);
        }

        for field in &plan.temp_exprs {
            let value = if field.is_temp_cacheable() {
                let cache_key = temp_cache_key(field);
                if let Some(value) = temp_cache.get(&cache_key) {
                    value.clone()
                } else {
                    let value = field.eval(state, &rows, &context, None).with_context(|| {
                        format!(
                            "rule {} temp field {} expression {} failed",
                            plan.rule.table_name, field.field.name, field.field.expression
                        )
                    })?;
                    temp_cache.insert(cache_key, value.clone());
                    value
                }
            } else {
                field.eval(state, &rows, &context, None).with_context(|| {
                    format!(
                        "rule {} temp field {} expression {} failed",
                        plan.rule.table_name, field.field.name, field.field.expression
                    )
                })?
            };
            context.insert(field.field.name.trim().to_string(), value);
        }

        let mut output = Row::new();
        for field in &plan.output_exprs {
            let value = field
                .eval(state, &rows, &context, Some(&output))
                .with_context(|| {
                    format!(
                        "rule {} output field {} expression {} failed",
                        plan.rule.table_name, field.field.name, field.field.expression
                    )
                })?;
            output.insert(field.field.name.trim().to_string(), value);
        }
        fill_output_time_from_context(&mut output, &context, row);
        let scan_start_time = output
            .get("scan_start_time")
            .context("output row missing scan_start_time")?;
        let output_values = ordered_output_values(&headers, &output);
        writer.write_values(scan_start_time, &output_values)?;
        Ok(())
    }
}

fn fill_output_time_from_context(output: &mut Row, context: &Row, row: &Row) {
    fill_output_time_field(output, context, row, "scan_start_time");
    fill_output_time_field(output, context, row, "scan_stop_time");
}

fn fill_output_time_field(output: &mut Row, context: &Row, row: &Row, field: &str) {
    if output
        .get(field)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
    {
        return;
    }
    if let Some(value) = find_row_value(context, field).or_else(|| find_row_value(row, field)) {
        output.insert(field.to_string(), value);
    } else if field.eq_ignore_ascii_case("scan_start_time") {
        if let Some(value) = find_row_value(context, "timestamp14(SOURCEFILENAME)") {
            output.insert(field.to_string(), value);
        }
    }
}

fn merge_projected_max_row(
    existing: &mut Row,
    row: &Row,
    required_fields: &FastHashSet<String>,
) -> Result<()> {
    for field in required_fields {
        let value = require_row_value(row, field)?;
        let Some(current) = existing.get_mut(field) else {
            existing.insert(field.clone(), value);
            continue;
        };
        if compare_max_value(&value, current) {
            *current = value;
        }
    }
    Ok(())
}

fn merge_projected_max_values(
    existing_values: &mut Option<Vec<String>>,
    existing_row: &mut Row,
    fields: &[String],
    values: &[String],
) -> Result<()> {
    let Some(existing_values) = existing_values else {
        return Ok(());
    };
    for (idx, candidate) in values.iter().enumerate() {
        if compare_max_value(candidate, &existing_values[idx]) {
            existing_values[idx] = candidate.clone();
            if let Some(field) = fields.get(idx) {
                existing_row.insert(field.clone(), candidate.clone());
            }
        }
    }
    Ok(())
}

fn project_row(row: &Row, required_fields: &FastHashSet<String>) -> Result<Row> {
    let mut projected = Row::new();
    for field in required_fields {
        projected.insert(field.clone(), require_row_value(row, field)?);
    }
    Ok(projected)
}

fn build_distinct(
    row: &Row,
    distinct_fields: &[String],
) -> Result<FastHashMap<String, FastHashSet<String>>> {
    let mut distinct: FastHashMap<String, FastHashSet<String>> = FastHashMap::default();
    for field in distinct_fields {
        let value = require_row_value(row, field)?;
        distinct.entry(field.clone()).or_default().insert(value);
    }
    Ok(distinct)
}

fn build_distinct_values(
    field_indexes: &FastHashMap<String, usize>,
    values: &[String],
    distinct_fields: &[String],
) -> Result<FastHashMap<String, FastHashSet<String>>> {
    let mut distinct: FastHashMap<String, FastHashSet<String>> = FastHashMap::default();
    for field in distinct_fields {
        let value = indexed_value(field_indexes, values, field)?.to_string();
        distinct.entry(field.clone()).or_default().insert(value);
    }
    Ok(distinct)
}

fn build_field_indexes(fields: &[String]) -> FastHashMap<String, usize> {
    fields
        .iter()
        .enumerate()
        .map(|(idx, field)| (normalize_lookup_name(field), idx))
        .collect()
}

fn sort_ordered_fields(fields: &mut [String]) {
    fields.sort_by(|left, right| {
        normalize_lookup_name(left)
            .cmp(&normalize_lookup_name(right))
            .then_with(|| left.cmp(right))
    });
}

fn row_from_values_ref(fields: &[String], values: &[String]) -> Row {
    let mut row = Row::with_capacity(fields.len());
    for (idx, field) in fields.iter().enumerate() {
        row.insert(field.clone(), values.get(idx).cloned().unwrap_or_default());
    }
    fill_standard_time_alias(&mut row, "scan_start_time", &["STARTTIME", "BEGINTIME"]);
    fill_standard_time_alias(&mut row, "scan_stop_time", &["ENDTIME"]);
    row
}

fn fill_standard_time_alias(row: &mut Row, standard: &str, aliases: &[&str]) {
    if row
        .get(standard)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
    {
        return;
    }
    let aliases: FastHashSet<String> = aliases
        .iter()
        .map(|alias| normalize_lookup_name(alias))
        .collect();
    let value = row.iter().find_map(|(field, value)| {
        if !value.is_empty() && aliases.contains(&normalize_lookup_name(field)) {
            Some(value.clone())
        } else {
            None
        }
    });
    if let Some(value) = value {
        row.insert(standard.to_string(), value);
    }
}

struct StreamingGroupState {
    row: Row,
    values: Option<Vec<String>>,
    distinct: FastHashMap<String, FastHashSet<String>>,
}

impl StreamingGroupState {
    fn new(row: Row) -> Self {
        Self {
            row,
            values: None,
            distinct: FastHashMap::default(),
        }
    }

    fn value_at(&self, idx: usize) -> Option<&str> {
        self.values
            .as_ref()
            .and_then(|values| values.get(idx))
            .map(String::as_str)
    }

    fn value_or_row(&self, field: &str, idx: usize) -> Result<String> {
        if let Some(value) = self.value_at(idx) {
            return Ok(value.to_string());
        }
        require_row_value(&self.row, field)
    }

    fn update(
        &mut self,
        row: &Row,
        distinct_fields: &[String],
        required_fields: &FastHashSet<String>,
    ) -> Result<()> {
        merge_projected_max_row(&mut self.row, row, required_fields)?;
        self.update_distinct(row, distinct_fields)
    }

    fn update_distinct(&mut self, row: &Row, distinct_fields: &[String]) -> Result<()> {
        for field in distinct_fields {
            let value = require_row_value(row, field)?;
            self.distinct
                .entry(field.clone())
                .or_default()
                .insert(value);
        }
        Ok(())
    }

    fn update_distinct_values(
        &mut self,
        field_indexes: &FastHashMap<String, usize>,
        values: &[String],
        distinct_fields: &[String],
    ) -> Result<()> {
        for field in distinct_fields {
            let value = indexed_value(field_indexes, values, field)?.to_string();
            self.distinct
                .entry(field.clone())
                .or_default()
                .insert(value);
        }
        Ok(())
    }
}

enum GroupKeyBuilder {
    Fields(Vec<String>),
    DnScanStartStop,
    DnTimestamp14Source,
    RdnTimestamp14Source,
    ObjectRdnScanStartStop,
    Expressions(Vec<String>),
}

impl GroupKeyBuilder {
    fn new(group_by: &[String]) -> Self {
        if group_by.len() == 2
            && group_by[0].eq_ignore_ascii_case("dn")
            && group_by[1].eq_ignore_ascii_case("timestamp14(SOURCEFILENAME)")
        {
            return Self::DnTimestamp14Source;
        }
        if group_by.len() == 2
            && group_by[0].eq_ignore_ascii_case("RDN")
            && group_by[1].eq_ignore_ascii_case("timestamp14(SOURCEFILENAME)")
        {
            return Self::RdnTimestamp14Source;
        }
        if group_by.len() == 3
            && group_by[0].eq_ignore_ascii_case("dn")
            && group_by[1].eq_ignore_ascii_case("scan_start_time")
            && group_by[2].eq_ignore_ascii_case("scan_stop_time")
        {
            return Self::DnScanStartStop;
        }
        if group_by.len() == 3
            && group_by[0].eq_ignore_ascii_case("object_rdn")
            && group_by[1].eq_ignore_ascii_case("scan_start_time")
            && group_by[2].eq_ignore_ascii_case("scan_stop_time")
        {
            return Self::ObjectRdnScanStartStop;
        }
        if group_by.iter().all(|field| is_simple_field_expr(field)) {
            return Self::Fields(group_by.to_vec());
        }
        Self::Expressions(group_by.to_vec())
    }

    fn build(&self, row: &Row) -> Result<String> {
        match self {
            Self::Fields(fields) => {
                join_key_values(fields.iter().map(|field| require_row_value(row, field)))
            }
            Self::DnTimestamp14Source => {
                let dn = require_row_value(row, "dn")?;
                let source = require_row_value(row, "SOURCEFILENAME")?;
                let scan_start = extract_timestamp14(&source)
                    .or_else(|| find_row_value(row, "scan_start_time"))
                    .unwrap_or_default();
                Ok(format!("{}\u{1f}{}", dn, scan_start))
            }
            Self::RdnTimestamp14Source => {
                let rdn = require_row_value(row, "RDN")?;
                let source = require_row_value(row, "SOURCEFILENAME")?;
                let scan_start = extract_timestamp14(&source)
                    .or_else(|| find_row_value(row, "scan_start_time"))
                    .unwrap_or_default();
                Ok(format!("{}\u{1f}{}", rdn, scan_start))
            }
            Self::DnScanStartStop => {
                let dn = require_row_value(row, "dn")?;
                let scan_start = require_row_value(row, "scan_start_time")?;
                let scan_stop = require_row_value(row, "scan_stop_time")?;
                Ok(format!("{}\u{1f}{}\u{1f}{}", dn, scan_start, scan_stop))
            }
            Self::ObjectRdnScanStartStop => {
                let object_rdn = require_row_value(row, "object_rdn")?;
                let scan_start = require_row_value(row, "scan_start_time")?;
                let scan_stop = require_row_value(row, "scan_stop_time")?;
                Ok(format!(
                    "{}\u{1f}{}\u{1f}{}",
                    object_rdn, scan_start, scan_stop
                ))
            }
            Self::Expressions(expressions) => {
                join_key_values(expressions.iter().map(|expr| eval_group_by_expr(row, expr)))
            }
        }
    }

    fn build_values(
        &self,
        field_indexes: &FastHashMap<String, usize>,
        values: &[String],
    ) -> Result<String> {
        match self {
            Self::Fields(fields) => join_key_values(fields.iter().map(|field| {
                indexed_value(field_indexes, values, field).map(|value| value.to_string())
            })),
            Self::DnTimestamp14Source => {
                let dn = indexed_value(field_indexes, values, "dn")?;
                let source = indexed_value(field_indexes, values, "SOURCEFILENAME")?;
                let scan_start = extract_timestamp14(source)
                    .or_else(|| optional_indexed_value(field_indexes, values, "scan_start_time"))
                    .unwrap_or_default();
                Ok(format!("{}\u{1f}{}", dn, scan_start))
            }
            Self::RdnTimestamp14Source => {
                let rdn = indexed_value(field_indexes, values, "RDN")?;
                let source = indexed_value(field_indexes, values, "SOURCEFILENAME")?;
                let scan_start = extract_timestamp14(source)
                    .or_else(|| optional_indexed_value(field_indexes, values, "scan_start_time"))
                    .unwrap_or_default();
                Ok(format!("{}\u{1f}{}", rdn, scan_start))
            }
            Self::DnScanStartStop => {
                let dn = indexed_value(field_indexes, values, "dn")?;
                let scan_start = indexed_value(field_indexes, values, "scan_start_time")?;
                let scan_stop = indexed_value(field_indexes, values, "scan_stop_time")?;
                Ok(format!("{}\u{1f}{}\u{1f}{}", dn, scan_start, scan_stop))
            }
            Self::ObjectRdnScanStartStop => {
                let object_rdn = indexed_value(field_indexes, values, "object_rdn")?;
                let scan_start = indexed_value(field_indexes, values, "scan_start_time")?;
                let scan_stop = indexed_value(field_indexes, values, "scan_stop_time")?;
                Ok(format!(
                    "{}\u{1f}{}\u{1f}{}",
                    object_rdn, scan_start, scan_stop
                ))
            }
            Self::Expressions(_) => bail!("expression group key requires row evaluation"),
        }
    }
}

fn indexed_value<'a>(
    field_indexes: &FastHashMap<String, usize>,
    values: &'a [String],
    field: &str,
) -> Result<&'a str> {
    let Some(idx) = field_indexes.get(&normalize_lookup_name(field)) else {
        bail!("missing field {field}");
    };
    Ok(values.get(*idx).map(String::as_str).unwrap_or_default())
}

fn optional_indexed_value(
    field_indexes: &FastHashMap<String, usize>,
    values: &[String],
    field: &str,
) -> Option<String> {
    let idx = field_indexes.get(&normalize_lookup_name(field))?;
    values.get(*idx).filter(|value| !value.is_empty()).cloned()
}

fn join_key_values<I>(values: I) -> Result<String>
where
    I: IntoIterator<Item = Result<String>>,
{
    let mut key = String::new();
    for value in values {
        if !key.is_empty() {
            key.push('\u{1f}');
        }
        key.push_str(&value?);
    }
    Ok(key)
}

struct CompiledFieldExpr<'a> {
    field: &'a FieldRule,
    kind: CompiledExpr,
}

impl<'a> CompiledFieldExpr<'a> {
    fn new(field: &'a FieldRule, field_indexes: &FastHashMap<String, usize>) -> Self {
        Self {
            field,
            kind: CompiledExpr::compile(&field.expression, field_indexes),
        }
    }

    fn eval(
        &self,
        state: &StreamingGroupState,
        rows: &[&Row],
        context: &Row,
        output: Option<&Row>,
    ) -> Result<String> {
        if let Some(value) = self.kind.eval(state, rows, context, output) {
            return value;
        }
        eval_stream_expression(&self.field.expression, state, rows, context, output)
    }

    fn is_temp_cacheable(&self) -> bool {
        self.kind.is_temp_cacheable()
    }
}

enum CompiledExpr {
    MaxIndex {
        field: String,
        idx: usize,
    },
    MaxField(String),
    LowerMaxIndex {
        field: String,
        idx: usize,
    },
    LowerMaxField(String),
    Crc64MaxIndex {
        field: String,
        idx: usize,
    },
    Crc64MaxField(String),
    Crc64LiteralMaxIndex {
        prefix: String,
        field: String,
        idx: usize,
    },
    Crc64LiteralMaxField {
        prefix: String,
        field: String,
    },
    CountDistinct(String),
    Literal(String),
    Env(String),
    CurrentTimestamp,
    FieldIndex {
        field: String,
        idx: usize,
    },
    Field(String),
    Fallback,
}

impl CompiledExpr {
    fn is_temp_cacheable(&self) -> bool {
        matches!(
            self,
            Self::MaxIndex { .. }
                | Self::MaxField(_)
                | Self::LowerMaxIndex { .. }
                | Self::LowerMaxField(_)
                | Self::Crc64MaxIndex { .. }
                | Self::Crc64MaxField(_)
                | Self::Crc64LiteralMaxIndex { .. }
                | Self::Crc64LiteralMaxField { .. }
                | Self::CountDistinct(_)
                | Self::Literal(_)
                | Self::Env(_)
        )
    }

    fn compile(expr: &str, field_indexes: &FastHashMap<String, usize>) -> Self {
        let expr = expr.trim();
        let lower = expr.to_ascii_lowercase();
        if expr.is_empty() {
            return Self::Literal(String::new());
        }
        if let Some(field) = simple_max_field(expr) {
            if let Some(idx) = field_index(field_indexes, field) {
                return Self::MaxIndex {
                    field: field.to_string(),
                    idx,
                };
            }
            return Self::MaxField(field.to_string());
        }
        if lower.starts_with("lower(") && expr.ends_with(')') {
            let inner = &expr[6..expr.len() - 1];
            if let Some(field) = simple_max_field(inner) {
                if let Some(idx) = field_index(field_indexes, field) {
                    return Self::LowerMaxIndex {
                        field: field.to_string(),
                        idx,
                    };
                }
                return Self::LowerMaxField(field.to_string());
            }
        }
        if lower.starts_with("crc64(") && expr.ends_with(')') {
            let inner = expr[6..expr.len() - 1].trim();
            if let Some(field) = simple_max_field(inner) {
                if let Some(idx) = field_index(field_indexes, field) {
                    return Self::Crc64MaxIndex {
                        field: field.to_string(),
                        idx,
                    };
                }
                return Self::Crc64MaxField(field.to_string());
            }
            if let Some((prefix, field)) = crc64_literal_plus_simple_max(inner) {
                if let Some(idx) = field_index(field_indexes, field) {
                    return Self::Crc64LiteralMaxIndex {
                        prefix,
                        field: field.to_string(),
                        idx,
                    };
                }
                return Self::Crc64LiteralMaxField {
                    prefix,
                    field: field.to_string(),
                };
            }
        }
        if lower.starts_with("count(distinct ") && expr.ends_with(')') {
            return Self::CountDistinct(expr[15..expr.len() - 1].trim().to_string());
        }
        if lower == "current_timestamp" {
            return Self::CurrentTimestamp;
        }
        if let Some(value) = parse_quoted_env(expr) {
            return Self::Env(value);
        }
        if let Some(value) = parse_quoted_literal(expr) {
            return Self::Literal(value);
        }
        if expr.eq_ignore_ascii_case("null") {
            return Self::Literal(String::new());
        }
        if lower.starts_with("case when ") {
            return Self::Fallback;
        }
        if is_simple_field_expr(expr) && expr.parse::<f64>().is_err() {
            if let Some(idx) = field_index(field_indexes, expr) {
                return Self::FieldIndex {
                    field: expr.to_string(),
                    idx,
                };
            }
            return Self::Field(expr.to_string());
        }
        Self::Fallback
    }

    fn eval(
        &self,
        state: &StreamingGroupState,
        rows: &[&Row],
        context: &Row,
        output: Option<&Row>,
    ) -> Option<Result<String>> {
        match self {
            Self::MaxIndex { field, idx } => Some(state.value_or_row(field, *idx)),
            Self::MaxField(field) => Some(require_row_value(rows[0], field)),
            Self::LowerMaxIndex { field, idx } => Some(
                state
                    .value_or_row(field, *idx)
                    .map(|value| value.to_ascii_lowercase()),
            ),
            Self::LowerMaxField(field) => {
                Some(require_row_value(rows[0], field).map(|value| value.to_ascii_lowercase()))
            }
            Self::Crc64MaxIndex { field, idx } => Some(
                state
                    .value_or_row(field, *idx)
                    .map(|value| crate::crc64::crc64_ecma(&value).to_string()),
            ),
            Self::Crc64MaxField(field) => Some(
                require_row_value(rows[0], field)
                    .map(|value| crate::crc64::crc64_ecma(&value).to_string()),
            ),
            Self::Crc64LiteralMaxIndex { prefix, field, idx } => {
                Some(state.value_or_row(field, *idx).map(|value| {
                    crate::crc64::crc64_ecma(&format!("{}{}", prefix, value)).to_string()
                }))
            }
            Self::Crc64LiteralMaxField { prefix, field } => {
                Some(require_row_value(rows[0], field).map(|value| {
                    crate::crc64::crc64_ecma(&format!("{}{}", prefix, value)).to_string()
                }))
            }
            Self::CountDistinct(field) => Some(Ok(state
                .distinct
                .get(field)
                .map(|values| values.len())
                .unwrap_or(0)
                .to_string())),
            Self::Literal(value) | Self::Env(value) => Some(Ok(value.clone())),
            Self::CurrentTimestamp => {
                Some(Ok(Local::now().format("%Y-%m-%d %H:%M:%S").to_string()))
            }
            Self::FieldIndex { field, idx } => Some(
                get_eval_context_value(context, output, field)
                    .map_or_else(|| state.value_or_row(field, *idx), Ok),
            ),
            Self::Field(field) => Some(
                get_eval_context_value(context, output, field)
                    .map_or_else(|| require_row_value(&state.row, field), Ok),
            ),
            Self::Fallback => None,
        }
    }
}

fn field_index(field_indexes: &FastHashMap<String, usize>, field: &str) -> Option<usize> {
    field_indexes.get(&normalize_lookup_name(field)).copied()
}

fn collect_count_distinct_fields(rule: &TpdRule) -> Vec<String> {
    let mut fields = Vec::new();
    for field in rule.temp_fields.iter().chain(rule.output_fields.iter()) {
        let expr = field.expression.trim();
        let lower = expr.to_ascii_lowercase();
        if lower.starts_with("count(distinct ") && expr.ends_with(')') {
            let inner = expr[15..expr.len() - 1].trim().to_string();
            if !fields.contains(&inner) {
                fields.push(inner);
            }
        }
    }
    fields
}

fn unique_output_headers(rule: &TpdRule) -> Vec<String> {
    let mut headers = Vec::new();
    let mut seen = FastHashSet::default();
    for field in &rule.output_fields {
        let name = field.name.trim().to_string();
        if seen.insert(name.clone()) {
            headers.push(name);
        }
    }
    headers
}

fn ordered_output_values(headers: &[String], output: &Row) -> Vec<String> {
    headers
        .iter()
        .map(|header| output.get(header).cloned().unwrap_or_default())
        .collect()
}

fn temp_cache_key(field: &CompiledFieldExpr<'_>) -> String {
    format!(
        "{}\u{1f}{}",
        normalize_lookup_name(&field.field.name),
        field.field.expression.trim()
    )
}

fn collect_required_source_fields(
    rule: &TpdRule,
    group: &GroupRule,
    distinct_fields: &[String],
) -> FastHashSet<String> {
    let mut fields = FastHashSet::default();
    for expr in &group.group_by {
        collect_expr_source_fields(expr, &mut fields);
    }
    collect_where_source_fields(&group.where_expr, &mut fields);
    for field in distinct_fields {
        fields.insert(field.clone());
    }
    let temp_names: FastHashSet<String> = rule
        .temp_fields
        .iter()
        .map(|field| normalize_lookup_name(&field.name))
        .collect();
    let output_names: FastHashSet<String> = rule
        .output_fields
        .iter()
        .map(|field| normalize_lookup_name(&field.name))
        .collect();
    for field in rule.temp_fields.iter().chain(rule.output_fields.iter()) {
        collect_rule_expr_source_fields(&field.expression, &mut fields, &temp_names, &output_names);
    }
    fields
}

fn collect_rule_expr_source_fields(
    expr: &str,
    fields: &mut FastHashSet<String>,
    temp_names: &FastHashSet<String>,
    output_names: &FastHashSet<String>,
) {
    let expr = expr.trim();
    let lower = expr.to_ascii_lowercase();
    if expr.is_empty()
        || lower == "null"
        || lower == "current_timestamp"
        || expr.parse::<f64>().is_ok()
        || parse_quoted_literal(expr).is_some()
    {
        return;
    }
    if let Some(field) = simple_max_field(expr) {
        fields.insert(field.to_string());
        return;
    }
    if lower.starts_with("lower(") && expr.ends_with(')') {
        if let Some(field) = simple_max_field(&expr[6..expr.len() - 1]) {
            fields.insert(field.to_string());
            return;
        }
    }
    if lower.starts_with("count(distinct ") && expr.ends_with(')') {
        fields.insert(expr[15..expr.len() - 1].trim().to_string());
        return;
    }
    if is_simple_field_expr(expr) {
        let normalized = normalize_lookup_name(expr);
        if !temp_names.contains(&normalized) && !output_names.contains(&normalized) {
            fields.insert(expr.to_string());
        }
        return;
    }
    collect_expr_source_fields_filtered(expr, fields, temp_names, output_names);
}

fn collect_expr_source_fields(expr: &str, fields: &mut FastHashSet<String>) {
    collect_expr_source_fields_filtered(
        expr,
        fields,
        &FastHashSet::default(),
        &FastHashSet::default(),
    );
}

fn collect_expr_source_fields_filtered(
    expr: &str,
    fields: &mut FastHashSet<String>,
    temp_names: &FastHashSet<String>,
    output_names: &FastHashSet<String>,
) {
    let mut token = String::new();
    let mut in_quote = false;
    for ch in expr.chars() {
        if ch == '\'' {
            in_quote = !in_quote;
            token.clear();
            continue;
        }
        if in_quote {
            continue;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            push_field_token(&token, fields, temp_names, output_names);
            token.clear();
        }
    }
    push_field_token(&token, fields, temp_names, output_names);
}

fn push_field_token(
    token: &str,
    fields: &mut FastHashSet<String>,
    temp_names: &FastHashSet<String>,
    output_names: &FastHashSet<String>,
) {
    if token.is_empty() || token.parse::<f64>().is_ok() {
        return;
    }
    let normalized = normalize_lookup_name(token);
    if temp_names.contains(&normalized) || output_names.contains(&normalized) {
        return;
    }
    let lower = token.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "max"
            | "lower"
            | "crc64"
            | "substring"
            | "locate"
            | "length"
            | "timestamp14"
            | "count"
            | "distinct"
            | "case"
            | "when"
            | "then"
            | "else"
            | "end"
            | "null"
            | "current_timestamp"
    ) {
        return;
    }
    fields.insert(token.to_string());
}

fn collect_where_source_fields(expr: &str, fields: &mut FastHashSet<String>) {
    let expr = expr.trim();
    if expr.is_empty() {
        return;
    }
    for part in expr.split(" and ") {
        let lower = part.to_ascii_lowercase();
        if let Some(idx) = lower.find(" like ") {
            fields.insert(part[..idx].trim().to_string());
        } else if let Some((left, _)) = part.split_once('=') {
            fields.insert(left.trim().to_string());
        }
    }
}

fn eval_stream_expression(
    expr: &str,
    state: &StreamingGroupState,
    rows: &[&Row],
    context: &Row,
    output: Option<&Row>,
) -> Result<String> {
    let expr = expr.trim();
    let lower = expr.to_ascii_lowercase();
    if let Some(field) = simple_max_field(expr) {
        return require_row_value(&state.row, field);
    }
    if lower.starts_with("lower(") && expr.ends_with(')') {
        let inner = &expr[6..expr.len() - 1];
        if let Some(field) = simple_max_field(inner) {
            return Ok(require_row_value(&state.row, field)?.to_ascii_lowercase());
        }
    }
    if lower.starts_with("crc64(") && expr.ends_with(')') {
        let inner = expr[6..expr.len() - 1].trim();
        if let Some(field) = simple_max_field(inner) {
            return Ok(
                crate::crc64::crc64_ecma(&require_row_value(&state.row, field)?).to_string(),
            );
        }
        if let Some((prefix, field)) = crc64_literal_plus_simple_max(inner) {
            let value = format!("{}{}", prefix, require_row_value(&state.row, field)?);
            return Ok(crate::crc64::crc64_ecma(&value).to_string());
        }
    }
    if lower.starts_with("count(distinct ") && expr.ends_with(')') {
        let inner = expr[15..expr.len() - 1].trim();
        return Ok(state
            .distinct
            .get(inner)
            .map(|values| values.len())
            .unwrap_or(0)
            .to_string());
    }
    eval_expression(expr, rows, context, output)
}

fn simple_max_field(expr: &str) -> Option<&str> {
    let expr = expr.trim();
    if !expr.to_ascii_lowercase().starts_with("max(") || !expr.ends_with(')') {
        return None;
    }
    let inner = expr[4..expr.len() - 1].trim();
    if is_simple_field_expr(inner) {
        Some(inner)
    } else {
        None
    }
}

fn crc64_literal_plus_simple_max(expr: &str) -> Option<(String, &str)> {
    let (left, right) = expr.split_once("||")?;
    let prefix = parse_quoted_literal(left.trim())?;
    let field = simple_max_field(right.trim())?;
    Some((prefix, field))
}

fn eval_where_expr(expr: &str, row: &Row) -> Result<bool> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Ok(true);
    }
    for part in expr.split(" and ") {
        let part = part.trim();
        let lower = part.to_ascii_lowercase();
        if let Some(idx) = lower.find(" like ") {
            let field = part[..idx].trim();
            let pattern = part[idx + 6..].trim();
            let Some(pattern) = parse_quoted_literal(pattern) else {
                bail!("unsupported where expression {expr}");
            };
            let value = require_row_value(row, field)?;
            if !like_matches(&value, &pattern) {
                return Ok(false);
            }
            continue;
        }
        if let Some((left, right)) = part.split_once('=') {
            let value = require_row_value(row, left.trim())?;
            let expected =
                parse_quoted_literal(right.trim()).unwrap_or_else(|| right.trim().to_string());
            if value != expected {
                return Ok(false);
            }
            continue;
        }
        bail!("unsupported where expression {expr}");
    }
    Ok(true)
}

fn like_matches(value: &str, pattern: &str) -> bool {
    if pattern.starts_with('%') && pattern.ends_with('%') && pattern.len() >= 2 {
        return value.contains(&pattern[1..pattern.len() - 1]);
    }
    if let Some(suffix) = pattern.strip_prefix('%') {
        return value.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('%') {
        return value.starts_with(prefix);
    }
    value == pattern
}

fn compare_max_value(candidate: &str, current: &str) -> bool {
    if candidate.is_empty() {
        return false;
    }
    if current.is_empty() {
        return true;
    }
    match (candidate.parse::<f64>(), current.parse::<f64>()) {
        (Ok(left), Ok(right)) => left > right,
        (Ok(_), Err(_)) => true,
        (Err(_), Ok(_)) => false,
        (Err(_), Err(_)) => candidate > current,
    }
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
        && !expr.contains('=')
        && !expr.contains('>')
        && !expr.contains('<')
        && !expr.contains(',')
        && !expr.contains('"')
        && !expr.contains('\'')
        && !expr.contains("||")
        && !expr.chars().any(char::is_whitespace)
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
    fn streaming_ordered_fields_include_lower_max_dependency() {
        let rule: TpdRule = serde_json::from_str(
            r#"{
              "table_name": "TPD_TEST",
              "groups": [{"name":"related_rdn01","enabled":true,"source_table":["OP_A"],"group_by":["dn","timestamp14(SOURCEFILENAME)"]}],
              "temp_fields": [{"name":"vendor_id_0","expression":"lower(max(VENDORNAME))","related_group":"related_rdn01"}],
              "output_fields": [{"name":"vendor_id","expression":"vendor_id_0"}]
            }"#,
        )
        .unwrap();

        let aggregator = StreamingRuleAggregator::new(&rule).unwrap();

        assert!(aggregator
            .ordered_fields
            .iter()
            .any(|field| field.eq_ignore_ascii_case("VENDORNAME")));
    }

    #[test]
    fn validate_streaming_rules_rejects_rule_without_one_enabled_group() {
        let rule: TpdRule = serde_json::from_str(
            r#"{
              "table_name":"TPD_BAD",
              "groups":[
                {"name":"g1","enabled":true,"source_table":"OP_A","group_by":["dn"]},
                {"name":"g2","enabled":true,"source_table":"OP_A","group_by":["dn"]}
              ],
              "temp_fields":[],
              "output_fields":[]
            }"#,
        )
        .unwrap();

        let err = validate_streaming_rules(&[rule]).unwrap_err();

        assert!(err.to_string().contains("TPD_BAD"));
        assert!(err.to_string().contains("streaming-compatible"));
        assert!(err
            .to_string()
            .contains("expected exactly one enabled group"));
    }

    #[test]
    fn streaming_value_path_binds_group_keys_and_simple_expressions_to_indexes() {
        let rule: TpdRule = serde_json::from_str(
            r#"{
              "table_name": "TPD_TEST",
              "groups": [{"name":"related_rdn01","enabled":true,"source_table":["OP_A"],"group_by":["dn","scan_start_time","scan_stop_time"]}],
              "temp_fields": [{"name":"lower_vendor","expression":"lower(max(VENDORNAME))","related_group":"related_rdn01"}],
              "output_fields": [
                {"name":"dn_out","expression":"max(dn)"},
                {"name":"vendor_crc","expression":"crc64(max(VENDORNAME))"},
                {"name":"vendor_lower","expression":"lower_vendor"},
                {"name":"nsa_count","expression":"count(distinct is_nsa)"}
              ]
            }"#,
        )
        .unwrap();

        let mut aggregator = StreamingRuleAggregator::new(&rule).unwrap();
        let dn_idx = aggregator.field_indexes[&normalize_lookup_name("dn")];
        let start_idx = aggregator.field_indexes[&normalize_lookup_name("scan_start_time")];
        let stop_idx = aggregator.field_indexes[&normalize_lookup_name("scan_stop_time")];
        let vendor_idx = aggregator.field_indexes[&normalize_lookup_name("VENDORNAME")];
        let nsa_idx = aggregator.field_indexes[&normalize_lookup_name("is_nsa")];

        let mut values = vec![String::new(); aggregator.ordered_fields.len()];
        values[dn_idx] = "dn-1".to_string();
        values[start_idx] = "2026-06-17 15:15:00".to_string();
        values[stop_idx] = "2026-06-17 15:30:00".to_string();
        values[vendor_idx] = "ZTE".to_string();
        values[nsa_idx] = "1".to_string();

        let key = aggregator
            .key_builder
            .build_values(&aggregator.field_indexes, &values)
            .unwrap();
        assert_eq!(
            key,
            "dn-1\u{1f}2026-06-17 15:15:00\u{1f}2026-06-17 15:30:00"
        );

        aggregator.accept_values(values).unwrap();
        let state = aggregator.grouped.values().next().unwrap();
        let plan = &aggregator.plans[0];
        let context = Row::new();
        let output = Row::new();

        assert_eq!(
            plan.temp_exprs[0].eval(state, &[], &context, None).unwrap(),
            "zte"
        );
        assert_eq!(
            plan.output_exprs[0]
                .eval(state, &[], &context, Some(&output))
                .unwrap(),
            "dn-1"
        );
        assert_eq!(
            plan.output_exprs[1]
                .eval(state, &[], &context, Some(&output))
                .unwrap(),
            crate::crc64::crc64_ecma("ZTE").to_string()
        );
        assert_eq!(
            plan.output_exprs[3]
                .eval(state, &[], &context, Some(&output))
                .unwrap(),
            "1"
        );
    }

    #[test]
    fn ordered_output_values_follow_unique_headers() {
        let headers = vec!["a".to_string(), "b".to_string()];
        let mut output = Row::new();
        output.insert("b".to_string(), "2".to_string());
        output.insert("a".to_string(), "1".to_string());

        assert_eq!(ordered_output_values(&headers, &output), vec!["1", "2"]);
    }

    #[test]
    fn temp_cache_key_requires_name_and_expression() {
        let left = FieldRule {
            name: "vendor_id_0".to_string(),
            expression: "lower(max(VENDORNAME))".to_string(),
        };
        let same = FieldRule {
            name: "vendor_id_0".to_string(),
            expression: "lower(max(VENDORNAME))".to_string(),
        };
        let different_name = FieldRule {
            name: "vendor_id_1".to_string(),
            expression: "lower(max(VENDORNAME))".to_string(),
        };
        let field_indexes = FastHashMap::default();
        let left = CompiledFieldExpr::new(&left, &field_indexes);
        let same = CompiledFieldExpr::new(&same, &field_indexes);
        let different_name = CompiledFieldExpr::new(&different_name, &field_indexes);

        assert_eq!(temp_cache_key(&left), temp_cache_key(&same));
        assert_ne!(temp_cache_key(&left), temp_cache_key(&different_name));
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
}
