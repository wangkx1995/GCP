# main.rs 模块化重构

## TL;DR

> **将 908 行 `main.rs` 拆分为 6 个模块文件 + 薄入口**
>
> **Deliverables**:
> - `src/crc64.rs` — CRC64 算法
> - `src/util.rs` — 通用工具函数
> - `src/config.rs` — mapping_dx.ini 配置解析
> - `src/parser.rs` — 文件解析、行富化
> - `src/tpd.rs` — TPD 规则数据结构与聚合执行
> - `src/writer.rs` — CSV 输出
> - `src/main.rs` — 精简为入口（~100 行）
>
> **Estimated Effort**: Medium
> **Parallel Execution**: YES — 6 waves
> **Critical Path**: crc64/util(独立) → config/parser → tpd/writer → main

---

## Context

### Original Request
将 main.rs 分模块重构，拆分 CRC64 / 配置解析 / 文件解析 / TPD 聚合 / CSV 输出 / 工具函数为独立文件。

### Current File Structure (main.rs, 908 行)

| 行号 | 内容 | 目标模块 |
|------|------|---------|
| 1-19 | use 导入、type Row/TableRows | main.rs 保留 |
| 21-37 | Cli 结构体 | main.rs 保留 |
| 41-51 | MappingConfig / ContextData | config.rs |
| 54-100 | TpdRule / GroupRule / FieldRule | tpd.rs |
| 104-150 | main() + collect_inputs | main.rs 保留 |
| 152-291 | parse_path / parse_csv | parser.rs |
| 293-365 | parse_mapping_config / read_config_text / split_mapping_line | config.rs |
| 367-437 | load_rule / execute_tpd_rule / merge_context | tpd.rs |
| 448-633 | eval_expression / eval_concat / eval_case_when / eval_condition / max_value / get_row_value / get_context_value / parse_quoted_literal / parse_quoted_env / java_crc64 / crc64_ecma | tpd.rs（crc64 除外→crc64.rs） |
| 655-725 | resolve_table / detect_counter_from_filename / lookup_source_value | config.rs |
| 728-763 | enrich_row / split_dn | parser.rs |
| 767-807 | write_tables / infer_headers | writer.rs |
| 810-893 | read_text / looks_like_delimited / normalize_value / column_name_format / normalize_lookup_name / file_name / strip_suffix / sanitize_file_name | util.rs |
| 896-908 | #[cfg(test)] mod tests（CRC64） | crc64.rs（内嵌 tests） |

### 模块依赖关系

```
main ──→ config ──→ parser ──→ tpd ──→ writer
  │         │                    │
  │         └── util ←───────────┤
  │                              │
  └── crc64 ─────────────────────┘
```

- **crc64.rs**: 无项目依赖（纯算法）
- **util.rs**: 无项目依赖（纯 std/encoding_rs 工具函数）
- **config.rs**: 依赖 `util::read_text`、`util::file_name`
- **parser.rs**: 依赖 `config::MappingConfig`/`ContextData`、`util::*`
- **tpd.rs**: 依赖 `crc64::crc64_ecma`、`util::*` 的 helper
- **writer.rs**: 依赖 `config::MappingConfig`、`util::normalize_value`

---

## Work Objectives

### Core Objective
将 908 行 main.rs 拆分为职责清晰的模块，main.rs 仅保留入口流程。

### Concrete Deliverables
- 6 个新模块文件（src/crc64.rs, util.rs, config.rs, parser.rs, tpd.rs, writer.rs）
- 重构后的 src/main.rs（~100 行）

### Definition of Done
- [ ] `cargo check --release` 通过
- [ ] `cargo test --release` 所有测试通过（CRC64 单测）
- [ ] 功能行为不变（无需运行真实数据）
- [ ] 重构后模块引用路径正确

### Must Have
- 每个模块有清晰的 `pub`/非 `pub` 可见性
- `Row` / `TableRows` 类型别名保留在 main.rs，模块通过 `crate::Row` 引用
- 无运行时行为变化

### Must NOT Have (Guardrails)
- 不改动任何业务逻辑
- 不改动函数签名（除非因可见性需要加 `pub`）
- 不添加新依赖
- 不重命名函数

---

## Verification Strategy

- **Automated tests**: YES (tests-after) — 使用 `cargo test --release` 验证
- **Agent QA**: `cargo check --release` + `cargo test --release` 在每个 Wave 结束时运行

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (独立模块 — 可完全并行):
├── Task 1: 创建 src/crc64.rs
├── Task 2: 创建 src/util.rs
└── [完成后编译验证]

Wave 2 (依赖 util 的模块):
├── Task 3: 创建 src/config.rs
├── Task 4: 创建 src/writer.rs
└── [完成后编译验证]

Wave 3 (依赖 config + util):
├── Task 5: 创建 src/parser.rs
├── Task 6: 创建 src/tpd.rs
└── [完成后编译验证]

Wave 4 (清理):
├── Task 7: 重构 src/main.rs 为入口层
└── [cargo check + cargo test]

Wave FINAL:
├── Task F1: 验证编译通过
├── Task F2: 确认无功能变更
└── git commit --amend 或新 commit
```

---

## TODOs

- [ ] 1. 创建 `src/crc64.rs`

  **What to do**:
  从 main.rs 提取 `java_crc64` (L608-649)、`crc64_ecma` (L651-653)、`static TABLE`，放入新文件。
  将 `use std::sync::OnceLock` 一起移入。
  将 `#[cfg(test)] mod tests` (L896-907) 移至本文件内，`use super::*` 改为 `use crate::crc64::crc64_ecma`。
  函数改为 `pub fn`。
  将所有内联 `const` (H1_INIT, H2_INIT, P1, P2) 提升为模块级 `const`，`TABLE` 提升为模块级 `static`。

  **Acceptance Criteria**:
  - [ ] `cargo check --release` 通过
  - [ ] `cargo test --release` 通过

  **QA Scenarios**:
  ```
  Scenario: 编译验证
    Tool: Bash
    Steps: cargo check --release && cargo test --release
    Expected: 编译成功，CRC64 测试通过
  ```

  **Commit**: YES
  - Message: `refactor: 拆分 crc64 模块`
  - Files: `src/crc64.rs` (new), `src/main.rs` (modified)

- [ ] 2. 创建 `src/util.rs`

  **What to do**:
  从 main.rs 提取以下函数到 `src/util.rs`，均标记 `pub`：
  - `read_text` (L810-818)
  - `looks_like_delimited` (L820-825)
  - `normalize_value` (L827-845)
  - `column_name_format` (L847-860)
  - `normalize_lookup_name` (L862-866)
  - `file_name` (L868-873)
  - `strip_suffix` (L875-881)
  - `sanitize_file_name` (L883-893)

  需要移入的 `use`：
  - `use std::fs::{self, File};`（仅 `File`）
  - `use std::io::Read;`
  - `use anyhow::Result;`
  - `use encoding_rs::GBK;`

  **注意**: `read_text` 用的是 `GBK`，`looks_like_delimited` 用了 `File` + `Read`。

  **Acceptance Criteria**:
  - [ ] `cargo check --release` 通过

  **Commit**: YES
  - Message: `refactor: 拆分 util 模块`
  - Files: `src/util.rs` (new), `src/main.rs` (modified)

- [ ] 3. 创建 `src/config.rs`

  **What to do**:
  从 main.rs 提取以下内容到 `src/config.rs`：
  - 结构体：`MappingConfig` (L41-44)、`ContextData` (L48-49)
  - `parse_mapping_config` (L293-327)
  - `split_mapping_line` (L360-365)
  - `resolve_table` (L655-676)
  - `detect_counter_from_filename` (L678-691)
  - `lookup_source_value` (L693-726)

  类型别名：在模块内 `use crate::{Row, TableRows};` 引用 main.rs 的类型。
  依赖：`use crate::util::{read_text, file_name};`

  **注意**: `resolve_table` 用了 `file_name`（在 util 中）、`detect_counter_from_filename` 也是。
  `Row` / `TableRows` 通过 `crate::` 导入。

  **Acceptance Criteria**:
  - [ ] `cargo check --release` 通过

  **Commit**: YES
  - Message: `refactor: 拆分 config 模块`
  - Files: `src/config.rs` (new), `src/main.rs` (modified), `src/util.rs` (可能需调整 pub)

- [ ] 4. 创建 `src/parser.rs`

  **What to do**:
  从 main.rs 提取以下函数到 `src/parser.rs`：
  - `parse_path` (L179-221) — `pub`
  - `parse_csv` (L224-290) — `pub`
  - `enrich_row` (L728-747) — `pub(crate)`
  - `split_dn` (L749-765) — `pub(crate)`

  需要 `use crate::{Row, TableRows, config::ContextData, util::*}`。
  `parse_csv` 中调用了 `resolve_table`、`detect_counter_from_filename`（config.rs 模块）、`enrich_row`、`normalize_value`、`normalize_lookup_name`、`column_name_format`（util.rs 模块）。

  **Acceptance Criteria**:
  - [ ] `cargo check --release` 通过

  **Commit**: YES
  - Message: `refactor: 拆分 parser 模块`
  - Files: `src/parser.rs` (new), `src/main.rs` (modified)

- [ ] 5. 创建 `src/tpd.rs`

  **What to do**:
  从 main.rs 提取以下内容到 `src/tpd.rs`：
  - 结构体：`TpdRule` (L54-57)、`GroupRule` (L63-82)、`FieldRule` (L84-100) — 全部 `pub`
  - `load_rule` (L367-371) — `pub`
  - `execute_tpd_rule` (L373-437) — `pub`
  - `merge_context` (L440-446) — `pub(crate)`
  - `eval_expression` (L448-499) — `pub(crate)`
  - `eval_concat_part` (L501-513) — `pub(crate)`
  - `eval_case_when` (L515-534) — `pub(crate)`
  - `eval_condition` (L536-543) — `pub(crate)`
  - `max_value` (L545-563) — `pub(crate)`
  - `get_row_value` (L565-576) — `pub(crate)`
  - `get_context_value` (L578-589) — `pub(crate)`
  - `parse_quoted_literal` (L591-597) — `pub(crate)`
  - `parse_quoted_env` (L599-605) — `pub(crate)`

  `crc64_ecma` 调用改为 `crate::crc64::crc64_ecma`。
  `use crate::{Row, TableRows, config::ContextData, util::*}`。

  **Acceptance Criteria**:
  - [ ] `cargo check --release` 通过

  **Commit**: YES
  - Message: `refactor: 拆分 tpd 模块`
  - Files: `src/tpd.rs` (new), `src/main.rs` (modified)

- [ ] 6. 创建 `src/writer.rs`

  **What to do**:
  从 main.rs 提取以下函数到 `src/writer.rs`：
  - `write_tables` (L767-795) — `pub`
  - `infer_headers` (L797-808) — `pub(crate)`

  需要 `use crate::{Row, TableRows, config::MappingConfig, util::normalize_value};`
  `write_tables` 调用了 `infer_headers`（本模块）、`normalize_value`（util）。

  **Acceptance Criteria**:
  - [ ] `cargo check --release` 通过

  **Commit**: YES
  - Message: `refactor: 拆分 writer 模块`
  - Files: `src/writer.rs` (new), `src/main.rs` (modified)

- [ ] 7. 重构 `src/main.rs` 为入口层

  **What to do**:
  main.rs 最终保留的内容：
  - `use` 导入（移走模块所需的 use）
  - `type Row = IndexMap<String, String>;`
  - `type TableRows = HashMap<String, Vec<Row>>;`
  - `struct Cli` (L21-37)
  - `fn main()` (L104-150)
  - `fn collect_inputs()` (L152-173) — 仅在 main 中调用
  - `mod crc64; mod util; mod config; mod parser; mod tpd; mod writer;`

  删除所有被提取出去的代码行。

  **最终 main.rs ≈ 100 行**。

  **Acceptance Criteria**:
  - [ ] `cargo check --release` 通过
  - [ ] `cargo test --release` 全部通过

  **Commit**: YES
  - Message: `refactor: 精简 main.rs 为入口，模块拆分完成`
  - Files: `src/main.rs` (modified)

---

## Final Verification Wave

- [ ] F1. **cargo check --release 通过**
- [ ] F2. **cargo test --release 通过**
- [ ] F3. **逻辑零改动验证**: `git diff main -- src/main.rs` 确认仅删除/移动了代码，没有新增业务逻辑
- [ ] F4. **提交所有改动**: `git commit -m "refactor: main.rs 模块化拆分完成"`

---

## Success Criteria

### Verification Commands
```bash
cargo check --release
cargo test --release
git status  # 确认所有改动文件
```

### Final Checklist
- [ ] 6 个新模块文件创建
- [ ] main.rs 从 908 行精简到 ~100 行
- [ ] 编译通过
- [ ] 单测通过
- [ ] 无业务逻辑变更
