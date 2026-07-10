# ADR-0001：结果网格使用数据完整性记录

## 状态

已决定

## 背景

原结果网格通过 `strategy_id + day` 查询 `collect_tasks`，并使用 `data_time`、`row_count` 和 `success` 构建按策略展示的每日网格。

新的业务目标是查看采集单元维度的数据处理结果。系统已经通过 `dal_data_integrity` 为 TPD 表和规则派生的 OP 表预建并回填完整性记录，因此该表是更符合业务语义的数据源。

## 决策

### 接口契约

直接替换现有 `GET /api/results/grid` 契约：

- 移除 `strategy_id` 查询参数。
- 使用 `collector_name` 和 `day` 查询。
- 不保留旧策略查询兼容逻辑，也不新增并行接口。
- 后端根据 `data_collector_unit.collector_interval` 确定网格周期。

### 查询数据源

结果记录从 `dal_data_integrity` 查询，条件为：

- `collector_name` 精确匹配。
- `scan_start_time` 属于所选日期。
- `period` 等于采集单元当前 `collector_interval`。

采集单元下拉选项从 `data_collector_unit` 获取，不从历史完整性记录去重生成。

### 预期行集合

后端使用采集单元当前配置推导完整网格行：

- 配置中的 TPD 表来自 `data_collector_unit.table_names`。
- OP 表通过当前 `config_version` 的规则推导。
- 行按 TPD 关系分组，每张 TPD 表后紧跟其 OP 表。

查询历史日期时仍使用当前配置，不恢复任务执行时的历史配置版本。

### 时间语义

- 横轴以 `scan_start_time` 为坐标。
- 日期按 `scan_start_time` 过滤。
- 时间窗结束后没有记录才判定为缺失。
- 尚未结束的时间窗视为未来，不显示颜色。

### 状态展示

- `task_status = 3` 且 `rows_num > 0`：绿色。
- `task_status = 3` 且 `rows_num = 0`：黄色。
- `task_status = 4`：红色。
- `task_status = 2`：蓝色。
- 已结束时间窗没有记录：灰色。
- 未结束时间窗：透明。

等待记录不根据 `task_timeout_seconds` 或记录年龄重新解释。

### 展示内容

单元格正文显示 `rows_num`。悬浮详情显示扫描时间窗、任务状态、实际行数、预期行数和完整率。预期行数与完整率不参与状态颜色判定。

## 影响

- 结果页的“策略”选择器改为“采集单元”选择器。
- 前后端 `GridQuery` 从 `strategy_id` 改为 `collector_name`。
- 网格返回结构需要携带扫描结束时间、预期行数和完整率，以支持悬浮详情。
- 原基于 `collect_tasks` 的结果网格查询不再对外提供。
- 当前配置变更可能改变历史日期的预期表集合及缺失判定，这是已接受的行为。

## 不在本次范围

- 不根据完整率改变颜色。
- 不将长期等待自动转换为超时或失败。
- 不允许选择历史 `period`。
- 不从已删除采集单元的历史记录生成下拉选项。
