# Domain Context

## 采集单元

采集单元由 `data_collector_unit` 定义，领域标识为 `unit_name`。在完整性数据中，对应字段为 `dal_data_integrity.collector_name`。

结果网格以采集单元为首要查询对象，而不是以采集策略为首要查询对象。

## 数据完整性记录

`dal_data_integrity` 中的一条记录表示：某个采集单元的一张 TPD 或 OP 表，在一个扫描时间窗内的任务状态与数据行数。

记录由以下字段共同标识：

- `collector_name`
- `table_name`
- `scan_start_time`
- `scan_end_time`
- `period`

## 扫描时间窗

扫描时间窗是从 `scan_start_time` 开始、到 `scan_end_time` 结束的采集区间。

- 结果网格横轴使用 `scan_start_time`。
- 日期筛选按 `scan_start_time` 所属日期执行。
- 网格列间隔使用采集单元当前配置的 `collector_interval`，单位为秒。
- 当天时间窗结束前属于未来窗口，不显示状态颜色。
- 时间窗结束后仍没有完整性记录时，该单元格为“缺失”。

## 预期表集合

预期表集合由采集单元当前配置推导：

1. 从 `data_collector_unit.table_names` 读取配置的 TPD 表。
2. 使用采集单元当前 `config_version` 对应的规则，为每张 TPD 表推导 OP 表。
3. 每张 TPD 表后紧跟其派生 OP 表，形成结果网格的行顺序。

历史日期同样使用当前配置推导预期表集合。配置变更后，历史网格会按当前配置重新解释缺失状态。

## 网格状态

结果网格主要表达任务状态，数据完整率不参与颜色判定。

| 条件 | 状态 | 颜色 |
| --- | --- | --- |
| `task_status = 3` 且 `rows_num > 0` | 成功且有数据 | 绿色 |
| `task_status = 3` 且 `rows_num = 0` | 成功但无数据 | 黄色 |
| `task_status = 4` | 失败 | 红色 |
| `task_status = 2` | 等待 | 蓝色 |
| 时间窗已结束但没有记录 | 缺失 | 灰色 |
| 时间窗尚未结束 | 未来 | 无颜色 |

`task_status = 2` 始终按等待状态展示，不根据记录存续时间自动重判为超时或失败。

## 网格单元格

单元格正文显示 `rows_num`。

悬浮详情显示：

- 扫描开始时间
- 扫描结束时间
- 任务状态
- 实际行数 `rows_num`
- 预期行数 `expected_rows_num`
- 完整率 `completion_rate`

`expected_rows_num` 和 `completion_rate` 仅用于详情展示，不影响单元格颜色。

## 查询交互

- 采集单元下拉选项来自 `data_collector_unit` 当前配置。
- 页面进入时不自动选择采集单元，也不自动查询。
- 日期默认当天。
- 周期只读展示采集单元当前 `collector_interval`。
- 用户选择采集单元后查询对应日期的完整性网格。
