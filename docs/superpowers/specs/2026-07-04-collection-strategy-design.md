# 采集策略管理 — 设计文档

## 概述

在 PM Admin 中增加采集策略管理功能，支持及时采集和周期性采集两种策略类型。一个策略对应一张表，选 N 张表生成 N 行策略。

## 数据模型

### `collection_strategy` 表

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | INTEGER PK AUTOINCREMENT | 采集策略Id |
| `collector_name` | TEXT NOT NULL | 采集单元名称（自动带出） |
| `collector_id` | INTEGER NOT NULL | 采集单元Id（自动带出） |
| `table_name` | TEXT NOT NULL | 单张表名，一个策略一行 |
| `status` | TEXT NOT NULL DEFAULT '可用' | `可用` / `挂起` |
| `cron_expression` | TEXT NOT NULL DEFAULT '' | Crontab 表达式（周期性） |
| `collect_interval` | INTEGER NOT NULL | 采集周期秒（自动带出） |
| `data_interval` | INTEGER NOT NULL | 数据周期秒（自动带出） |
| `data_start_time` | TEXT | 数据开始时间（及时） |
| `data_end_time` | TEXT | 数据结束时间（及时） |
| `execute_time` | TEXT | 执行时间（及时） |
| `agent_ids` | TEXT NOT NULL DEFAULT '[]' | 采集机 JSON 数组 |
| `strategy_type` | TEXT NOT NULL | `immediate` / `periodic` |
| `created_at` | TEXT NOT NULL | |
| `updated_at` | TEXT NOT NULL | |

遵循现有模式：`agent_ids` 存 JSON 字符串，序列化时解析为数组。

## API 接口

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/strategies/next-id` | 预分配 ID |
| POST | `/api/strategies` | 创建（传入 `table_names[]` 拆成多行） |
| GET | `/api/strategies?collector_id=&type=&status=` | 列表查询 |
| GET | `/api/strategies/:id` | 单条详情 |
| PUT | `/api/strategies/:id` | 修改 |
| POST | `/api/strategies/batch-suspend` | 批量挂起 `{ ids: [1,2,3] }` |
| POST | `/api/strategies/batch-activate` | 批量激活 `{ ids: [1,2,3] }` |

### 创建请求体

```json
{
  "collector_id": 1,
  "collector_name": "机房A-北向指标",
  "table_names": ["TPD_EUTRANCELL_0_Q", "TPD_NRCELL_0_Q"],
  "cron_expression": "0 0 * * *",
  "collect_interval": 900,
  "data_interval": 900,
  "data_start_time": null,
  "data_end_time": null,
  "execute_time": null,
  "agent_ids": ["agent_local"],
  "strategy_type": "periodic"
}
```

后端接收 `table_names` 数组，每个元素生成一行 `collection_strategy`。

### 列表响应

每行一条策略，包含所有字段（密码脱敏不需处理，没有密码字段）。

## 前端页面

### 路由

| 路径 | 页面 |
|------|------|
| `/strategy-dispatch/info` | 列表页（采集策略信息） |
| `/strategy-dispatch/immediate` | 新建及时采集策略 |
| `/strategy-dispatch/immediate/:id/edit` | 编辑及时采集策略 |
| `/strategy-dispatch/periodic` | 新建周期性采集策略 |
| `/strategy-dispatch/periodic/:id/edit` | 编辑周期性采集策略 |

### 采集策略信息（列表页）

- 表格列：`□` 复选框 + 策略Id + 采集单元名称 + 表名 + 类型(Tag) + Crontab + 采集机 + 状态(Tag, 绿色=可用/灰色=挂起) + 操作(编辑/挂起/激活)
- 批量操作栏：选中后显示"批量挂起"/"批量激活"
- 操作列挂起/激活按钮根据状态显示（可用→显示挂起，挂起→显示激活）
- 点击行进入编辑
- 接口预留 `collector_name` 查询参数，列表页暂不加筛选栏

### 及时采集策略（新建表单）

- 选择采集单元（Select/搜索，复用现有 hook `useDataCollectorUnits`）
  - 选择后自动带出：`collector_name`, `collector_id`, `collect_interval`, `data_interval`, `agent_ids`
- 指标组（多选 Select，从已选采集单元的 `table_names` 加载可用列表）
- 数据开始时间（DatePicker）
- 数据结束时间（DatePicker）
- 执行时间（DatePicker）
- 采集机（多选 Select，默认已选，可修改）
- 保存按钮 → 调用 POST `/api/strategies` → 提示成功 → 跳转到列表页

### 周期性采集策略（新建表单）

- 选择采集单元 → 自动带出字段
- 指标组（多选）
- Crontab 表达式（Input）
- 采集机（多选，默认已选，可修改）
- 保存按钮 → 调用 POST `/api/strategies` → 提示成功 → 跳转到列表页

### 编辑

编辑页面复用新建表单，ID 存在则走编辑模式：
- 采集单元、表名不可修改
- 可修改：Crontab/执行参数、采集机、状态

## 实现计划

1. DB migration + CoreDb CRUD（`strategies.rs` 或内联到 `db.rs`）
2. HTTP handler in `server.rs`
3. 共享类型 in `core_agent_api.rs`
4. Frontend: types + API + hooks
5. Frontend: list page
6. Frontend: immediate strategy form
7. Frontend: periodic strategy form
