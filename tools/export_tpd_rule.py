#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

import xlrd


def cell(row, idx):
    if idx >= len(row):
        return ""
    value = row[idx]
    if isinstance(value, float) and value.is_integer():
        return str(int(value))
    return str(value).strip()


def rows_after(sheet, header_name):
    for row_idx in range(sheet.nrows):
        row = sheet.row_values(row_idx)
        if cell(row, 0) == header_name:
            headers = [cell(row, col) for col in range(sheet.ncols)]
            data = []
            for data_idx in range(row_idx + 1, sheet.nrows):
                values = sheet.row_values(data_idx)
                if cell(values, 0) in {
                    "index_name",
                    "dest_table",
                    "group_name",
                    "table_group_name",
                    "tmp_store_field",
                    "store_field",
                }:
                    break
                if not any(cell(values, col) for col in range(min(sheet.ncols, 10))):
                    continue
                item = {headers[col]: cell(values, col) for col in range(len(headers)) if headers[col]}
                data.append(item)
            return data
    return []


def split_csv(value):
    return [part.strip() for part in value.split(",") if part.strip()]


def field_rule(row, name_key):
    return {
        "name": row.get(name_key, "").strip(),
        "field_cn": row.get("field_name_cn", "").strip(),
        "field_eng": row.get("field_name_eng", "").strip(),
        "data_type": row.get("field_datatype", "").strip(),
        "constraint": row.get("Constraint", "").strip(),
        "default_value": row.get("default_value", "").strip(),
        "expression": row.get("exp_select", "").strip(),
        "related_group": row.get("related_rdn", "").strip(),
        "description": row.get("description", "").strip(),
    }


def export_rule(workbook_path, sheet_name):
    book = xlrd.open_workbook(workbook_path, formatting_info=False)
    sheet = book.sheet_by_name(sheet_name)

    groups = []
    for row in rows_after(sheet, "group_name"):
        if not row.get("group_name") or row.get("table_group_name") == "self":
            continue
        groups.append(
            {
                "name": row.get("table_group_name", "").strip(),
                "group_name": row.get("group_name", "").strip(),
                "enabled": row.get("group_flag", "") not in {"", "0", "0.0"},
                "source_table": row.get("exp_from", "").strip(),
                "where_expr": row.get("exp_where", "").strip(),
                "group_by": split_csv(row.get("exp_groupby", "")),
                "order_by": split_csv(row.get("exp_orderby", "")),
                "join_keys": split_csv(row.get("exp_join", "")),
                "transform_type": row.get("transform_type", "").strip(),
            }
        )

    temp_fields = [field_rule(row, "tmp_store_field") for row in rows_after(sheet, "tmp_store_field")]
    output_fields = [field_rule(row, "store_field") for row in rows_after(sheet, "store_field")]

    return {
        "table_name": sheet_name,
        "groups": groups,
        "temp_fields": [field for field in temp_fields if field["name"]],
        "output_fields": [field for field in output_fields if field["name"]],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--workbook", required=True)
    parser.add_argument("--sheet", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    rule = export_rule(args.workbook, args.sheet)
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(rule, ensure_ascii=False, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
