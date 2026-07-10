use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::Serialize;

use crate::core_agent_api::{IntegrityRow, ResultRow};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DailyGrid {
    pub day: String,
    pub time_slots: Vec<String>,
    pub rows: Vec<TableGridRow>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TableGridRow {
    pub table_name: String,
    pub cells: Vec<GridCell>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct GridCell {
    pub data_time: String,
    pub value: Option<u64>,
    pub color: String,
    pub status: String,
    pub scan_end_time: String,
    pub expected_rows_num: Option<u64>,
    pub completion_rate: Option<f64>,
}

pub fn build_daily_grid(day: &str, interval_minutes: u32, expected_tables: &[String], rows: &[ResultRow]) -> DailyGrid {
    let slots = time_slots(day, interval_minutes);
    let mut tables: BTreeSet<String> = expected_tables.iter().cloned().collect();
    for row in rows {
        tables.insert(row.table_name.clone());
    }
    let by_key: BTreeMap<(String, String), &ResultRow> = rows.iter().map(|row| ((row.table_name.clone(), row.data_time.clone()), row)).collect();
    let rows = tables.into_iter().map(|table_name| {
        let cells = slots.iter().map(|slot| {
            if let Some(row) = by_key.get(&(table_name.clone(), slot.clone())) {
                let (color, status) = if row.success == 0 {
                    ("red", "failed")
                } else if row.row_count == 0 {
                    ("yellow", "empty")
                } else {
                    ("green", "ok")
                };
                GridCell {
                    data_time: slot.clone(),
                    value: Some(row.row_count),
                    color: color.to_string(),
                    status: status.to_string(),
                    scan_end_time: String::new(),
                    expected_rows_num: None,
                    completion_rate: None,
                }
            } else {
                GridCell {
                    data_time: slot.clone(),
                    value: None,
                    color: "gray".to_string(),
                    status: "missing".to_string(),
                    scan_end_time: String::new(),
                    expected_rows_num: None,
                    completion_rate: None,
                }
            }
        }).collect();
        TableGridRow { table_name, cells }
    }).collect();
    DailyGrid { day: day.to_string(), time_slots: slots, rows }
}

pub fn build_integrity_grid(
    day: &str,
    period_seconds: u32,
    expected_tables: &[String],
    rows: &[IntegrityRow],
    now: &str,
) -> Result<DailyGrid> {
    let slots = time_slots_seconds(day, period_seconds)?;
    let by_key: BTreeMap<(String, String), &IntegrityRow> = rows
        .iter()
        .map(|row| ((row.table_name.clone(), row.scan_start_time.clone()), row))
        .collect();
    let mut seen_tables = BTreeSet::new();
    let rows = expected_tables
        .iter()
        .filter(|table_name| seen_tables.insert((*table_name).clone()))
        .map(|table_name| {
            let cells = slots
                .iter()
                .map(|slot| {
                    if let Some(row) = by_key.get(&(table_name.clone(), slot.clone())) {
                        let (color, status) = match row.task_status {
                            3 if row.rows_num > 0 => ("green", "ok"),
                            3 => ("yellow", "empty"),
                            4 => ("red", "failed"),
                            2 => ("blue", "waiting"),
                            _ => ("gray", "unknown"),
                        };
                        GridCell {
                            data_time: slot.clone(),
                            value: Some(row.rows_num),
                            color: color.to_string(),
                            status: status.to_string(),
                            scan_end_time: row.scan_end_time.clone(),
                            expected_rows_num: Some(row.expected_rows_num),
                            completion_rate: Some(row.completion_rate),
                        }
                    } else {
                        let scan_end_time = add_seconds(slot, period_seconds);
                        let (color, status) = if scan_end_time.as_str() > now {
                            ("none", "future")
                        } else {
                            ("gray", "missing")
                        };
                        GridCell {
                            data_time: slot.clone(),
                            value: None,
                            color: color.to_string(),
                            status: status.to_string(),
                            scan_end_time,
                            expected_rows_num: None,
                            completion_rate: None,
                        }
                    }
                })
                .collect();
            TableGridRow { table_name: table_name.clone(), cells }
        })
        .collect();
    Ok(DailyGrid { day: day.to_string(), time_slots: slots, rows })
}

fn time_slots(day: &str, interval_minutes: u32) -> Vec<String> {
    let mut slots = Vec::new();
    let mut minute = 0;
    while minute < 24 * 60 {
        slots.push(format!("{} {:02}:{:02}:00", day, minute / 60, minute % 60));
        minute += interval_minutes;
    }
    slots
}

fn time_slots_seconds(day: &str, period_seconds: u32) -> Result<Vec<String>> {
    if period_seconds == 0 {
        anyhow::bail!("采集周期必须大于 0 秒");
    }
    let mut slots = Vec::new();
    let mut second = 0;
    while second < 24 * 60 * 60 {
        slots.push(format!(
            "{} {:02}:{:02}:{:02}",
            day,
            second / 3600,
            second % 3600 / 60,
            second % 60
        ));
        second += period_seconds;
    }
    Ok(slots)
}

fn add_seconds(date_time: &str, seconds: u32) -> String {
    let value = chrono::NaiveDateTime::parse_from_str(date_time, "%Y-%m-%d %H:%M:%S")
        .expect("网格时间格式应有效");
    (value + chrono::Duration::seconds(seconds as i64))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_agent_api::IntegrityRow;

    #[test]
    fn builds_daily_grid_with_missing_and_success_cells() {
        let grid = build_daily_grid(
            "2026-06-17",
            15,
            &["TPD_A".to_string()],
            &[ResultRow { table_name: "TPD_A".to_string(), data_time: "2026-06-17 00:15:00".to_string(), row_count: 7, success: 1, collect_time: "2026-07-02 15:35:00".to_string() }],
        );
        assert_eq!(grid.time_slots.len(), 96);
        assert_eq!(grid.rows.len(), 1);
        assert_eq!(grid.rows[0].cells[0].color, "gray");
        assert_eq!(grid.rows[0].cells[1].value, Some(7));
        assert_eq!(grid.rows[0].cells[1].color, "green");
    }

    #[test]
    fn builds_integrity_grid_with_task_status_colors_and_details() {
        let rows = vec![
            IntegrityRow {
                table_name: "TPD_A".to_string(),
                scan_start_time: "2026-07-09 00:00:00".to_string(),
                scan_end_time: "2026-07-09 00:15:00".to_string(),
                rows_num: 12,
                expected_rows_num: 10,
                completion_rate: 1.2,
                task_status: 3,
            },
            IntegrityRow {
                table_name: "TPD_A".to_string(),
                scan_start_time: "2026-07-09 00:15:00".to_string(),
                scan_end_time: "2026-07-09 00:30:00".to_string(),
                rows_num: 0,
                expected_rows_num: 12,
                completion_rate: 0.0,
                task_status: 3,
            },
            IntegrityRow {
                table_name: "OP_A".to_string(),
                scan_start_time: "2026-07-09 00:00:00".to_string(),
                scan_end_time: "2026-07-09 00:15:00".to_string(),
                rows_num: 0,
                expected_rows_num: 0,
                completion_rate: 0.0,
                task_status: 4,
            },
            IntegrityRow {
                table_name: "OP_A".to_string(),
                scan_start_time: "2026-07-09 00:15:00".to_string(),
                scan_end_time: "2026-07-09 00:30:00".to_string(),
                rows_num: 0,
                expected_rows_num: 0,
                completion_rate: 0.0,
                task_status: 2,
            },
        ];

        let grid = build_integrity_grid(
            "2026-07-09",
            900,
            &["TPD_A".to_string(), "OP_A".to_string()],
            &rows,
            "2026-07-10 00:00:00",
        ).unwrap();

        assert_eq!(grid.rows[0].table_name, "TPD_A");
        assert_eq!(grid.rows[1].table_name, "OP_A");
        assert_eq!(grid.rows[0].cells[0].color, "green");
        assert_eq!(grid.rows[0].cells[1].color, "yellow");
        assert_eq!(grid.rows[1].cells[0].color, "red");
        assert_eq!(grid.rows[1].cells[1].color, "blue");
        assert_eq!(grid.rows[0].cells[0].scan_end_time, "2026-07-09 00:15:00");
        assert_eq!(grid.rows[0].cells[0].expected_rows_num, Some(10));
        assert_eq!(grid.rows[0].cells[0].completion_rate, Some(1.2));
    }

    #[test]
    fn builds_integrity_grid_with_unique_table_rows() {
        let grid = build_integrity_grid(
            "2026-07-09",
            900,
            &[
                "TPD_NRCELLDU_PRB_Q_5G".to_string(),
                "OP_NRCELLDU".to_string(),
                "Tpd_NRCELLDU_q_5g".to_string(),
                "OP_NRCELLDU".to_string(),
            ],
            &[],
            "2026-07-10 00:00:00",
        ).unwrap();

        assert_eq!(
            grid.rows.iter().map(|row| row.table_name.as_str()).collect::<Vec<_>>(),
            vec!["TPD_NRCELLDU_PRB_Q_5G", "OP_NRCELLDU", "Tpd_NRCELLDU_q_5g"]
        );
    }

    #[test]
    fn rejects_zero_period_when_building_integrity_grid() {
        let result = build_integrity_grid(
            "2026-07-09",
            0,
            &["TPD_A".to_string()],
            &[],
            "2026-07-10 00:00:00",
        );

        assert!(result.is_err());
    }
}
