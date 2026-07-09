# ScanWindow 计算实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Rust 周期调度模块中实现 `get_scan_scope`，根据 period 网格和 delay_period 计算 `[scan_start_time, scan_end_time]`，并接入 `periodic_strategy_scan_loop`。

**Architecture:** 新增纯函数模块 `src/scan_window.rs` 负责时间窗计算；`collection_strategy` 表与 API 增加 `delay_period` 字段；`src/core/server.rs` 的周期扫描循环调用该模块生成 `scan_start_time` / `scan_end_time` 并随任务下发；前端策略表单增加 `delay_period` 输入。

**Tech Stack:** Rust, chrono, sqlx, tokio, axum, React + Ant Design

## Global Constraints

- 所有时间计算按北京时间（UTC+8，`FixedOffset::east_opt(8 * 3600)`）处理。
- `period <= 0` 返回 `None`，不触发采集。
- `delay_period` 默认 0，单位秒，不允许负数；非法值按 0 处理。
- `period == 2592000` 为日历月分支，窗宽不按固定 30 天，而是“上月 1 号 → 本月 1 号”。
- 周期扫描循环保持现有 cron 触发逻辑不变；本功能只替换时间窗计算部分。
- DB 静态 SQL 使用 `trace_sql!` 宏；动态 SQL 使用 `tracing::info!`。
- 新增字段必须同步 `collection_strategy` 建表、迁移、查询、插入、更新、结构体、前端类型。

---

### Task 1: 扩展数据模型 —— 新增 `delay_period` 字段

**Files:**
- Modify: `src/core_agent_api.rs:307-355`
- Modify: `src/core/db.rs:347-386`（CREATE TABLE）
- Modify: `src/core/db.rs:534-565`（migration / ALTER TABLE）
- Modify: `src/core/db.rs:827-868`（insert）
- Modify: `src/core/db.rs:873-881`（get）
- Modify: `src/core/db.rs:894-920`（list）
- Modify: `src/core/db.rs:923-980`（update）
- Modify: `src/core/db.rs:1016-1028`（list_active_periodic_strategies）
- Modify: `src/core/db.rs:2000-2090`（unit tests）

**Interfaces:**
- Consumes: `CollectionStrategyCreateRequest.delay_period: i64`, `CollectionStrategyUpdateRequest.delay_period: Option<i64>`
- Produces: `CollectionStrategyRow.delay_period: i64`

- [ ] **Step 1: 修改 `core_agent_api.rs` 结构体**

在 `CollectionStrategyRow` 中 `data_interval` 后增加：

```rust
    pub delay_period: i64,
```

在 `CollectionStrategyCreateRequest` 中 `data_interval` 后增加：

```rust
    pub delay_period: i64,
```

在 `CollectionStrategyUpdateRequest` 中 `data_interval` 后增加：

```rust
    pub delay_period: Option<i64>,
```

- [ ] **Step 2: 修改 `src/core/db.rs` 建表语句**

在 `CREATE TABLE collection_strategy` 的 `data_interval INTEGER NOT NULL` 后增加：

```sql
                    delay_period INTEGER NOT NULL DEFAULT 0,
```

在迁移旧数据时，新表插入语句增加一列 `delay_period`，并在 `rows.push(CollectionStrategyRow { ... })` 中增加 `delay_period: 0,`。

- [ ] **Step 3: 修改所有 SQL 查询/插入/更新语句**

将所有 `SELECT ... data_interval, data_start_time ...` 改为 `SELECT ... data_interval, delay_period, data_start_time ...`。
将 `INSERT` 语句增加 `delay_period` 列并 `.bind(req.delay_period.max(0))`。
在 `rows.push(CollectionStrategyRow { ... })` 中增加 `delay_period: req.delay_period.max(0),`。
在 `update_strategy` 动态 SQL 中增加：

```rust
        if let Some(v) = req.delay_period {
            sql.push_str(", delay_period = ?");
            values.push(v.max(0).to_string());
        }
```

- [ ] **Step 4: 运行 DB 单元测试**

Run:

```bash
cargo test collection_strategy_crud -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/core_agent_api.rs src/core/db.rs
git commit -m "feat(db): add delay_period to collection_strategy"
```

---

### Task 2: 任务下发接口增加 `scan_end_time`

**Files:**
- Modify: `src/core_agent_api.rs:390-430`
- Modify: `src/core/server.rs:863-890` 附近 `TaskDispatchRequest` 构造处

**Interfaces:**
- Consumes: `StrategyCommand.scan_end_time: Option<String>`
- Produces: `TaskDispatchRequest.scan_end_time: Option<String>`

- [ ] **Step 1: 修改 `TaskDispatchRequest`**

在 `scan_start_time` 后增加：

```rust
    pub scan_end_time: Option<String>,
```

- [ ] **Step 2: 修改 `src/core/server.rs` 中构造 `TaskDispatchRequest` 的位置**

找到 `TaskDispatchRequest { ... }` 初始化处（约 540-570 行），增加：

```rust
    scan_end_time: command.scan_end_time.clone(),
```

- [ ] **Step 3: 编译检查**

Run:

```bash
cargo check
```

Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add src/core_agent_api.rs src/core/server.rs
git commit -m "feat(api): pass scan_end_time in task dispatch"
```

---

### Task 3: 实现 `src/scan_window.rs` 纯函数模块

**Files:**
- Create: `src/scan_window.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: fire time ms, period sec, delay sec
- Produces: `pub fn get_scan_scope(fire_time_ms: i64, period_sec: i64, delay_sec: i64) -> Option<(i64, i64)>`

- [ ] **Step 1: 创建 `src/scan_window.rs`**

```rust
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, TimeZone, Timelike};

const BEIJING_OFFSET_SECS: i32 = 8 * 3600;
const MINUTE_MS: i64 = 60_000;

fn beijing() -> FixedOffset {
    FixedOffset::east_opt(BEIJING_OFFSET_SECS).unwrap()
}

/// 计算扫描时间窗 [start, end]（毫秒）
pub fn get_scan_scope(fire_time_ms: i64, period_sec: i64, delay_sec: i64) -> Option<(i64, i64)> {
    if period_sec <= 0 {
        return None;
    }
    let delay_sec = delay_sec.max(0);
    let fire_min = (fire_time_ms / MINUTE_MS) * MINUTE_MS;
    let delay_ms = delay_sec * 1000;

    if period_sec == 2_592_000 {
        let aligned = align_month(fire_min);
        let start = prev_month_first_day_ms(aligned) - delay_ms;
        let end = aligned - delay_ms;
        return Some((start, end));
    }

    let aligned = match period_sec {
        3_600 => align_hour(fire_min),
        86_400 => align_day(fire_min),
        604_800 => align_week(fire_min),
        _ if period_sec >= 3_600 => align_to_period_grid(fire_min, period_sec),
        _ => align_to_period(fire_min, period_sec),
    };

    let period_ms = period_sec * 1000;
    let end = aligned - delay_ms;
    let start = aligned - period_ms - delay_ms;
    Some((start, end))
}

/// 亚小时周期：按北京时间 +8h 对齐后取模 floor
fn align_to_period(t: i64, period_sec: i64) -> i64 {
    let p = period_sec * 1000;
    let shift = BEIJING_OFFSET_SECS as i64 * 1000;
    t - ((t + shift) % p)
}

/// 长周期（>=3600 非精确值）：直接按周期毫秒取模 floor
fn align_to_period_grid(t: i64, period_sec: i64) -> i64 {
    let p = period_sec * 1000;
    t - (t % p)
}

fn align_hour(t: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(t).unwrap().with_timezone(&beijing());
    beijing()
        .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), dt.hour(), 0, 0)
        .unwrap()
        .timestamp_millis()
}

fn align_day(t: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(t).unwrap().with_timezone(&beijing());
    beijing()
        .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
        .unwrap()
        .timestamp_millis()
}

fn align_week(t: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(t).unwrap().with_timezone(&beijing());
    let days_since_monday = dt.weekday().num_days_from_monday();
    let date = NaiveDate::from_ymd_opt(dt.year(), dt.month(), dt.day()).unwrap()
        - chrono::Duration::days(days_since_monday as i64);
    beijing()
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
        .unwrap()
        .timestamp_millis()
}

fn align_month(t: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(t).unwrap().with_timezone(&beijing());
    beijing()
        .with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
        .unwrap()
        .timestamp_millis()
}

fn prev_month_first_day_ms(current_month_first_ms: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(current_month_first_ms)
        .unwrap()
        .with_timezone(&beijing());
    let year = dt.year();
    let month = dt.month();
    let (prev_year, prev_month) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    beijing()
        .with_ymd_and_hms(prev_year, prev_month, 1, 0, 0, 0)
        .unwrap()
        .timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(h: u32, m: u32, s: u32) -> i64 {
        beijing()
            .with_ymd_and_hms(2026, 7, 9, h as i32, m as i32, s as i32)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn period_zero_returns_none() {
        assert_eq!(get_scan_scope(ts(5, 0, 0), 0, 0), None);
        assert_eq!(get_scan_scope(ts(5, 0, 0), -1, 0), None);
    }

    #[test]
    fn fifteen_minutes_no_delay() {
        let (start, end) = get_scan_scope(ts(5, 0, 0), 900, 0).unwrap();
        assert_eq!(start, ts(4, 45, 0));
        assert_eq!(end, ts(5, 0, 0));
    }

    #[test]
    fn fifteen_minutes_with_delay() {
        let (start, end) = get_scan_scope(ts(5, 0, 0), 900, 300).unwrap();
        assert_eq!(start, ts(4, 40, 0));
        assert_eq!(end, ts(4, 55, 0));
    }

    #[test]
    fn two_hour_grid() {
        let (start, end) = get_scan_scope(ts(5, 30, 0), 7200, 0).unwrap();
        assert_eq!(start, ts(2, 0, 0));
        assert_eq!(end, ts(4, 0, 0));
    }

    #[test]
    fn one_hour() {
        let (start, end) = get_scan_scope(ts(5, 23, 0), 3600, 0).unwrap();
        assert_eq!(start, ts(4, 0, 0));
        assert_eq!(end, ts(5, 0, 0));
    }

    #[test]
    fn one_day() {
        let (start, end) = get_scan_scope(ts(9, 12, 0), 86400, 0).unwrap();
        assert_eq!(end, ts(0, 0, 0));
        assert_eq!(start, ts(0, 0, 0) - 86400 * 1000);
    }

    #[test]
    fn one_week_monday() {
        // 2026-07-09 is Thursday; Monday is 2026-07-06
        let (start, end) = get_scan_scope(ts(9, 12, 0), 604800, 0).unwrap();
        assert_eq!(end, beijing().with_ymd_and_hms(2026, 7, 6, 0, 0, 0).unwrap().timestamp_millis());
        assert_eq!(start, end - 604800 * 1000);
    }

    #[test]
    fn calendar_month() {
        // fire = 2026-07-09 12:00, aligned = 2026-07-01, start = 2026-06-01
        let (start, end) = get_scan_scope(ts(9, 12, 0), 2_592_000, 0).unwrap();
        let expected_end = beijing().with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap().timestamp_millis();
        let expected_start = beijing().with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap().timestamp_millis();
        assert_eq!(end, expected_end);
        assert_eq!(start, expected_start);
    }
}
```

- [ ] **Step 2: 注册模块**

在 `src/lib.rs` 中增加：

```rust
pub mod scan_window;
```

如果 `src/lib.rs` 不存在或结构不同，改为在 `src/main.rs`、`src/bin/core.rs`、`src/bin/agent.rs` 中使用 `mod scan_window;`（根据实际入口）。本项目三入口共享 `src/lib.rs`，因此只需在 `src/lib.rs` 暴露。

- [ ] **Step 3: 运行新增单元测试**

Run:

```bash
cargo test scan_window -- --nocapture
```

Expected: 8 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/scan_window.rs src/lib.rs
git commit -m "feat(scan_window): implement get_scan_scope with period alignment"
```

---

### Task 4: 接入周期扫描循环

**Files:**
- Modify: `src/core/server.rs:1330-1356`

**Interfaces:**
- Consumes: `CollectionStrategyRow.delay_period`, `CollectionStrategyRow.data_interval`, `crate::timeutil::now()`
- Produces: `StrategyCommand.scan_start_time`, `StrategyCommand.scan_end_time`

- [ ] **Step 1: 替换旧的时间窗计算逻辑**

找到 `periodic_strategy_scan_loop` 中如下代码：

```rust
            let now = crate::timeutil::now();
            let interval = strategy.data_interval.max(60);
            let ts = now.timestamp();
            let rem = ts % interval;
            let scan_start_time = chrono::DateTime::from_timestamp(ts - rem - interval, 0)
                .map(|t| t.naive_utc().format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| now.format("%Y-%m-%d %H:%M:%S").to_string());
```

替换为：

```rust
            let now = crate::timeutil::now();
            let fire_time_ms = now.timestamp_millis();
            let period_sec = strategy.data_interval.max(60);
            let delay_sec = strategy.delay_period.max(0);
            let (scan_start_ms, scan_end_ms) = match crate::scan_window::get_scan_scope(fire_time_ms, period_sec, delay_sec) {
                Some(v) => v,
                None => {
                    tracing::warn!(strategy_id = %strategy.strategy_id, period = period_sec, "invalid period, skip periodic strategy");
                    continue;
                }
            };
            let fmt = |ms: i64| {
                chrono::DateTime::from_timestamp_millis(ms)
                    .map(|t| t.with_timezone(&crate::timeutil::offset()).format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| now.format("%Y-%m-%d %H:%M:%S").to_string())
            };
            let scan_start_time = fmt(scan_start_ms);
            let scan_end_time = Some(fmt(scan_end_ms));
```

注意：`crate::timeutil::offset()` 当前未暴露，需要先在 `src/timeutil.rs` 中将 `offset()` 改为 `pub fn offset()`。

- [ ] **Step 2: 修改 `StrategyCommand` 构造**

将原：

```rust
                scan_end_time: strategy.data_end_time.clone(),
```

改为：

```rust
                scan_end_time,
```

- [ ] **Step 3: 暴露 `timeutil::offset()`**

在 `src/timeutil.rs` 中：

```rust
pub fn offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}
```

删除或重命名原来的私有 `fn offset()`。

- [ ] **Step 4: 编译检查**

Run:

```bash
cargo check
```

Expected: no errors

- [ ] **Step 5: 运行核心测试**

Run:

```bash
cargo test --bin core -- --nocapture
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/core/server.rs src/timeutil.rs
git commit -m "feat(core): use get_scan_scope in periodic scan loop"
```

---

### Task 5: 前端周期策略表单增加 `delay_period` 输入

**Files:**
- Modify: `pm-admin/src/pages/StrategyDispatch/PeriodicStrategy.tsx`
- Modify: `pm-admin/src/types/api.ts`（实际类型定义文件，路径以项目为准）

**Interfaces:**
- Consumes: API `CollectionStrategyRow.delay_period`
- Produces: Form value `delay_period`

- [ ] **Step 1: 更新前端类型**

在 `CollectionStrategyCreateRequest` / `CollectionStrategyUpdateRequest` / `CollectionStrategy` 类型定义中增加：

```typescript
  delay_period?: number;
```

- [ ] **Step 2: 在周期策略表单中增加输入项**

在 `pm-admin/src/pages/StrategyDispatch/PeriodicStrategy.tsx` 表单合适位置（建议放在 `data_interval` 附近）增加：

```tsx
<Form.Item
  label="回溯周期（秒）"
  name="delay_period"
  rules={[{ type: 'number', min: 0, message: '不能为负数' }]}
  initialValue={0}
>
  <InputNumber min={0} placeholder="0" style={{ width: '100%' }} />
</Form.Item>
```

并确保提交对象 `data` 包含 `delay_period: values.delay_period ?? 0`。

- [ ] **Step 3: 构建前端**

Run:

```bash
cd pm-admin && npm run build
```

Expected: build success

- [ ] **Step 4: Commit**

```bash
git add pm-admin/src
git commit -m "feat(admin): add delay_period field to periodic strategy form"
```

---

### Task 6: 全量验证

**Files:**
- All modified above

- [ ] **Step 1: 运行 Rust 全量测试**

Run:

```bash
cargo test
```

Expected: full suite PASS（当前约 62 个单元测试）

- [ ] **Step 2: 发布构建**

Run:

```bash
cargo build --release --locked
```

Expected: success

- [ ] **Step 3: 复制二进制到测试目录（按 AGENTS.md）**

Run:

```bash
cp target/release/core test/core && \
cp target/release/agent test/agent && \
cp server.toml agent.toml test/
```

Expected: files copied

- [ ] **Step 4: 前端 lint**

Run:

```bash
cd pm-admin && npm run lint
```

Expected: no new errors（原有 `FormPage.tsx` warning 可忽略，除非修改了该文件）

- [ ] **Step 5: Commit 并标记完成**

```bash
git add -A
git commit -m "test: verify scan window feature end-to-end"
```

---

## Self-Review Checklist

**1. Spec coverage:**
- `period <= 0` 返回 null → Task 3 测试 + Task 4 处理
- 亚小时周期取模兜底 +8h → Task 3 `align_to_period`
- 3600/86400/604800/2592000 特殊分支 → Task 3 独立函数
- `>=3600` 非整值 floor 到周期网格 → Task 3 `align_to_period_grid`
- `delay_period` 字段新增 → Task 1 + Task 5
- 周期扫描循环接入 → Task 4
- `scan_end_time` 下发 → Task 2 + Task 4

**2. Placeholder scan:** 本计划无 TBD/TODO/"实现 later" 等占位。

**3. Type consistency:**
- `delay_period` 在 Rust 中为 `i64`，前端为 `number`，DB 为 `INTEGER`。
- `scan_end_time` 在 `StrategyCommand`、`TaskGroup`、`TaskDispatchRequest` 中均为 `Option<String>`。
