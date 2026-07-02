use std::path::Path;

use anyhow::Result;
use walkdir::WalkDir;

use crate::core_agent_api::ResultRow;

pub fn read_result_rows(output_dir: &Path) -> Result<Vec<ResultRow>> {
    let mut rows = Vec::new();
    for entry in WalkDir::new(output_dir).into_iter().filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_file() || entry.file_name() != "result.csv" {
            continue;
        }
        let mut reader = csv::Reader::from_path(entry.path())?;
        for record in reader.records() {
            let record = record?;
            rows.push(ResultRow {
                table_name: record.get(0).unwrap_or_default().to_string(),
                data_time: record.get(1).unwrap_or_default().to_string(),
                row_count: record.get(2).unwrap_or("0").parse::<u64>()?,
                success: record.get(3).unwrap_or("0").parse::<i32>()?,
                collect_time: record.get(4).unwrap_or_default().to_string(),
            });
        }
    }
    rows.sort_by(|left, right| left.table_name.cmp(&right.table_name).then(left.data_time.cmp(&right.data_time)));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reads_nested_result_csv_rows() {
        let dir = tempdir().unwrap();
        let package_dir = dir.path().join("tpd_a_2026061715").join("collect_1_202606171515");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(package_dir.join("result.csv"), "table_name,data_time,row_count,success,collect_time\nTPD_A,2026-06-17 15:15:00,100,1,2026-07-02 15:35:00\n").unwrap();

        let rows = read_result_rows(dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].table_name, "TPD_A");
        assert_eq!(rows[0].row_count, 100);
    }
}
