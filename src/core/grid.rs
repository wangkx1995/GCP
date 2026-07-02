use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::core_agent_api::ResultRow;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DailyGrid {
    pub day: String,
    pub time_slots: Vec<String>,
    pub rows: Vec<TableGridRow>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TableGridRow {
    pub table_name: String,
    pub cells: Vec<GridCell>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct GridCell {
    pub data_time: String,
    pub value: Option<u64>,
    pub color: String,
    pub status: String,
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
                GridCell { data_time: slot.clone(), value: Some(row.row_count), color: color.to_string(), status: status.to_string() }
            } else {
                GridCell { data_time: slot.clone(), value: None, color: "gray".to_string(), status: "missing".to_string() }
            }
        }).collect();
        TableGridRow { table_name, cells }
    }).collect();
    DailyGrid { day: day.to_string(), time_slots: slots, rows }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
