# Core/Agent 采集系统测试指南

## 准备工作

项目根目录执行：

```bash
export https_proxy=http://127.0.0.1:7890 http_proxy=http://127.0.0.1:7890
```

## 步骤一：验证现有解析器正常工作

用本地文件跑一遍现有 CLI，确保解析流程可用。

### 1. 创建测试数据目录

```bash
mkdir -p test_data/input test_data/output
```

### 2. 创建 mapping_dx.ini

```ini
[tablemapping]
op_test = dest_result

[dest_result]
_id = INT
dn = TEXT
result_time = TEXT
counter_value = TEXT
```

### 3. 创建 load.toml

```toml
[clickhouse]
client = "clickhouse-client"
host = "127.0.0.1"
port = 9000
user = "default"
password = ""
database = "default"
table_name_case = "lower"

[postgresql]
client = "psql"
host = "127.0.0.1"
port = 5432
user = "postgres"
password = ""
database = "postgres"
```

### 4. 创建测试 PM CSV

```bash
cat > test_data/input/OP_TEST_20260617151500.csv << 'EOF'
_id,dn,result_time,counter_value
1,PLMN=1/LNBTS=1/LNCELL=1,2026-06-17 15:15:00,1000
2,PLMN=1/LNBTS=1/LNCELL=2,2026-06-17 15:15:00,2000
EOF
```

### 5. 创建规则文件

```bash
mkdir -p test_data/rules
cat > test_data/rules/rule_a.json << 'EOF'
{
  "table_name": "DEST_RESULT",
  "groups": [
    {
      "name": "g1",
      "enabled": true,
      "source_table": "OP_TEST",
      "group_by": ["dn", "result_time"]
    }
  ],
  "temp_fields": [],
  "output_fields": [
    {"name": "dn", "expression": "max(dn)"},
    {"name": "result_time", "expression": "max(result_time)"},
    {"name": "row_count", "expression": "count(distinct dn)"},
    {"name": "total_counter", "expression": "max(counter_value)"}
  ]
}
EOF
```

### 6. 执行解析

```bash
cargo run -- --input test_data/input \
  --config-dir test_data \
  --output-dir test_data/output \
  --collect-id test_collect_001 \
  --load-type clickhouse \
  --load-config test_data/load.toml \
  --rules-dir test_data/rules \
  --output-delimiter "|"
```

期望输出：`[done] X.XXs total`，出现 `[write] 字样。检查输出目录：

```bash
find test_data/output -type f
```

应能看到类似结构：

```text
test_data/output/dest_result_2026061715/test_collect_001_202606171515/dest_result.csv
test_data/output/dest_result_2026061715/test_collect_001_202606171515/dest_result.ini
test_data/output/dest_result_2026061715/test_collect_001_202606171515/load.ctl
test_data/output/dest_result_2026061715/test_collect_001_202606171515/result.csv
```

查看 `result.csv`：

```bash
cat test_data/output/dest_result_2026061715/test_collect_001_202606171515/result.csv
```

```csv
table_name,data_time,row_count,success,collect_time
DEST_RESULT,2026-06-17 15:15:00,2,1,YYYY-MM-DD HH:MM:SS
```

步骤一验证了解析器本身工作正常。后续步骤验证 Core/Agent 系统。

## 步骤二：Core + Agent 启动和注册

### 1. 构建所有二进制

```bash
cargo build --bin core --bin agent
```

### 2. 启动 Core（终端 A）

```bash
cargo run --bin core -- --listen 127.0.0.1:18080 --db core.db
```

### 3. 启动 Agent（终端 B）

```bash
cargo run --bin agent -- \
  --listen 127.0.0.1:18081 \
  --data-dir agent_data \
  --core-api-base http://127.0.0.1:18080/api \
  --agent-id agent_local
```

### 4. 注册 Agent（终端 C）

```bash
curl -sS -X POST http://127.0.0.1:18080/api/agents/register \
  -H 'content-type: application/json' \
  -d '{
    "agent_id": "agent_local",
    "agent_name": "agent-local",
    "host": "127.0.0.1",
    "port": 18081,
    "version": "1.0.0",
    "capabilities": {
      "can_collect": true,
      "can_parse": true,
      "can_load": false,
      "supported_protocols": ["ftp", "sftp", "local"]
    }
  }'
```

期望返回：

```json
{
  "agent_id": "agent_local",
  "heartbeat_interval_seconds": 10,
  "task_report_interval_seconds": 10
}
```

## 步骤三：下发采集任务到 Agent

当前 Agent 使用 `--source-config` 模式，需要准备 `source.toml` 和对应配置。

### 1. 在 Agent 的 task 目录创建配置

```bash
TASK_DIR="agent_data/tasks/task_001"
mkdir -p "$TASK_DIR/config/rules"
mkdir -p "$TASK_DIR/downloads"
mkdir -p "$TASK_DIR/output"
mkdir -p "$TASK_DIR/logs"
```

### 2. 复制配置文件

```bash
cp test_data/mapping_dx.ini "$TASK_DIR/config/"
cp test_data/load.toml "$TASK_DIR/config/"
cp test_data/rules/rule_a.json "$TASK_DIR/config/rules/"
```

### 3. 创建 source.toml

如果你有 FTP/SFTP 源，创建实际的 `source.toml`：

```toml
[source]
type = "ftp"
download_dir = "agent_data/tasks/task_001/downloads"
remote_pattern = "OP_TEST_.*\\.csv$"
connect_retry = 3
download_retry = 2
download_parallel = 1
retry_interval_secs = 5
connect_timeout_secs = 10
read_timeout_secs = 30
cache_retention_days = 7

[source.connection]
host = "your-ftp-host"
port = 21
username = "user"
password = "pass"
```

**本地测试代替方案：** 如果你没有 FTP/SFTP 源，也可以修改测试方式：

```bash
# 把测试文件复制到 downloads 目录
cp test_data/input/OP_TEST_20260617151500.csv agent_data/tasks/task_001/downloads/

# 创建本地模式的 source.toml（注意：当前仅支持 ftp/sftp 类型）
# 所以本地测试需要临时绕过；建议直接用 CLI 模式验证解析
```

### 4. 下发任务

```bash
curl -sS -X POST http://127.0.0.1:18081/api/tasks \
  -H 'content-type: application/json' \
  -d '{
    "task_id": "task_001",
    "logical_task_key": "strategy:2026-06-17-15:15:00:cfg",
    "strategy_id": "strategy_1",
    "config_snapshot_id": "cfg_001",
    "scan_start_time": "2026-06-17 15:15:00",
    "collect_id": "test_collect_001",
    "load_type": "clickhouse",
    "encoding": "UTF-8",
    "output_delimiter": "|",
    "timeout_seconds": 1800,
    "callback_base_url": "http://127.0.0.1:18080/api"
  }'
```

期望返回：

```json
{
  "task_id": "task_001",
  "accepted": true,
  "agent_task_state": "ACCEPTED",
  "reason": null
}
```

### 5. 查询 Agent 本地状态

```bash
cat agent_data/tasks/task_001/task.json
cat agent_data/tasks/task_001/state.json
```

## 步骤四：查询采集结果 Grid

```bash
curl -sS 'http://127.0.0.1:18080/api/results/grid?strategy_id=strategy_1&day=2026-06-17&interval_minutes=15' | python3 -m json.tool
```

返回示例：

```json
{
  "day": "2026-06-17",
  "time_slots": [
    "2026-06-17 00:00:00",
    "2026-06-17 00:15:00",
    "...",
    "2026-06-17 23:45:00"
  ],
  "rows": [
    {
      "table_name": "DEST_RESULT",
      "cells": [
        {"data_time": "2026-06-17 00:00:00", "value": null, "color": "gray", "status": "missing"},
        {"data_time": "2026-06-17 00:15:00", "value": null, "color": "gray", "status": "missing"},
        {"data_time": "2026-06-17 15:15:00", "value": 2, "color": "green", "status": "ok"},
        {"data_time": "2026-06-17 15:30:00", "value": null, "color": "gray", "status": "missing"},
        "..." 
      ]
    }
  ]
}
```

颜色含义：

| 颜色 | 含义 |
|------|------|
| 绿色 | 采集成功，有数据 |
| 黄色 | 采集成功，0 行数据 |
| 红色 | 采集失败 |
| 灰色 | 未采集到该时间点 |

## 常见问题排查

**问题 1：Agent 不接任务**
检查 Agent 是否已启动并能访问 Core。查看 Agent 日志。

**问题 2：任务下发后状态不变**
当前 Agent runner 使用 `--source-config` 模式，需要真实的 FTP/SFTP 源。如果你想用 `--input` 模式本地测试，需要修改 `runner.rs` 中的 `run_task` 方法，将 `input: None, source_config: Some(...)` 改为 `input: Some(task_dir.join("downloads")), source_config: None`。

**问题 3：Core 返回 500**
检查 Core 日志和 `core.db` 文件。可以用 SQLite 查看：

```bash
sqlite3 core.db ".tables"
sqlite3 core.db "SELECT * FROM agents"
sqlite3 core.db "SELECT * FROM collect_tasks"
sqlite3 core.db "SELECT * FROM collect_result_cells"
```

**问题 4：配置快照接口返回错误**
当前 `GET /api/config-snapshots/{id}` 是桩接口。配置快照的管理和 Agent 自动拉取功能在二期实现。测试阶段需手动创建配置文件。

## 快速回滚

```bash
# 停掉 Core 和 Agent（Ctrl+C）
rm -rf agent_data core.db test_data/output
# 重新开始
```
