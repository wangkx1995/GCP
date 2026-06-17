# 远程源输入模块设计文档

## 概述

`crates/remote-file-source` 是独立远程文件获取库，负责根据配置确定数据文件来源路径。该库支持三类输入源：

- **本地文件** (`local`)：直接读取本地 CSV/CSV.gz 文件。
- **FTP 远程文件** (`ftp`)：连接 FTP 服务器扫描目录、正则匹配、流式下载后再导入。
- **SFTP 远程文件** (`sftp`)：连接 SFTP 服务器扫描目录、正则匹配、流式下载后再导入。

无论远程还是本地，最终对外暴露的接口始终是一个 `Vec<PathBuf>` 列表，由上层调用者逐个导入。远程文件会先下载到本地缓存目录，再复用现有 PM 解析、TPD 聚合和输出包流程。

当前 PM 解析器通过以下参数选择输入模式：

- `--input`：本地文件或目录模式。
- `--source-config source.toml`：FTP/SFTP 远程模式。
- `--scan-start-time "yyyy-MM-dd HH:mm:ss"`：远程模式必传，用于渲染 `${SCAN_START_TIME,...}`。

`--input` 和 `--source-config` 互斥；远程模式缺少 `--scan-start-time` 时直接停止采集。

---

## 外部入口

### `remote_file_source::resolve_files(options) -> Result<Vec<PathBuf>>`

这是模块的唯一公开函数。处理逻辑：

1. `local_input` 有值时走本地文件/目录模式。
2. `source_config` 有值时走远程模式，根据 `source.type` 分发：
   - `"ftp"` → FTP 扫描、匹配、下载。
   - `"sftp"` → SFTP 扫描、匹配、下载。
3. `local_input` 和 `source_config` 同时存在时报错。
4. 两者都不存在时报错。

---

## 远程文件解析流程

当 `source.type = "ftp" | "sftp"` 时，执行以下完整流程：

### 流程概览

```
resolve_remote_files()
   │
   ├── load_source_config()           ← 读取 TOML 并校验配置完整性
   ├── create_dir_all(download_dir)    ← 确保本地缓存目录存在
   ├── cleanup_download_dir()         ← 下载前清理过期缓存文件
   ├── require scan_start_time        ← 远程模式必须由 CLI 传入
   ├── render_scan_start_time()       ← 渲染模板为日期字符串
   ├── Regex::new(rendered)           ← 编译正则
   ├── infer_scan_dir()               ← 推导扫描目录
   ├── log_remote_context()           ← 打印完整上下文日志
   ├── list_*_files()                  ← 连接远程服务器，递归扫描
   │   ├── ensure_*_scan_dir()        ← 预检查目录可访问性
   │   └── list_*_recursive()         ← 实际递归列文件
   ├── retain + sort                  ← 正则匹配 + 字典序排序
   ├── 匹配数为 0? → 详细错误退出
   └── download_*_files()             ← 逐个流式下载
       ├── download_*_file()          ← 单文件下载（.part → rename）
       └── finalize + rename
```

### 1. 配置校验 (`load_source_config`)

校验内容：

```
- source.remote_pattern 不能为空
- source.download_dir 不能为空
- source.connect_retry 必须大于 0，默认 3
- source.download_retry 必须大于 0，默认 3
- source.download_parallel 必须大于 0，默认 1
- source.retry_interval_secs 必须大于 0，默认 30
- source.connect_timeout_secs 必须大于 0，默认 30
- source.read_timeout_secs 必须大于 0，默认 300
- source.connection.host 不能为空
- source.connection.username 不能为空
- source.connection.password 不能为空
```

校验失败时返回明确字段名，例如 `source.retry_interval_secs must be greater than 0`。

### 2. 扫描开始时间解析

远程模式只接受 CLI 参数 `--scan-start-time`，配置文件中不提供兜底时间。缺少该参数时直接报错并停止采集。

时间格式支持：

- `yyyy-MM-dd HH:mm:ss` → 作为本地无时区时间解析

### 3. 路径模板渲染 (`render_scan_start_time`)

将 `source.remote_pattern` 中的 `${SCAN_START_TIME,格式}` 替换为实际日期字符串。也支持在 `SCAN_START_TIME` 后追加时间偏移。

`source.remote_pattern` 支持用英文分号 `;` 配置多个完整远程路径正则模板。每个模板会独立渲染、推导扫描目录、扫描和匹配。

#### 支持的格式

| 模板格式 | 示例输出 |
|----------|----------|
| `yyyyMMdd` | `20260210` |
| `yyyy-MM-dd` | `2026-02-10` |
| `yyyyMMddHH` | `2026021012` |
| `yyyyMMddHHmm` | `202602101200` |
| `yyyyMMddHHmmss` | `20260210120000` |

#### 支持的偏移

| 偏移写法 | 含义 |
|----------|------|
| `+15m` | 增加 15 分钟 |
| `-15m` | 减少 15 分钟 |
| `+1h` | 增加 1 小时 |
| `-1h` | 减少 1 小时 |
| `+1d` | 增加 1 天 |
| `-1d` | 减少 1 天 |

#### 渲染行为

- 使用 `Regex` 匹配 `${SCAN_START_TIME([+-]\d+[mhd])?,format}`。
- 非贪婪匹配，支持单模板中出现多个 `${SCAN_START_TIME,...}`。
- 不支持的格式或偏移语法会返回明确的错误。

#### 示例

```
模板：   /data/WX/PA/${SCAN_START_TIME,yyyyMMdd}/FILE_${SCAN_START_TIME,yyyyMMdd}120000.*.csv
渲染后： /data/WX/PA/20260210/FILE_20260210120000.*.csv

模板：   /data/WX/PA/${SCAN_START_TIME,yyyyMMdd}/FILE_${SCAN_START_TIME+15m,yyyyMMddHHmm}.*.csv
渲染后： /data/WX/PA/20260210/FILE_202602101215.*.csv
```

### 4. 正则编译 (`Regex::new`)

渲染后的完整字符串直接作为正则表达式编译。这意味着：

- `.` 字符在正则语义中匹配任意字符，除非在 `remote_pattern` 中转义为 `\\.`。
- `.*` 匹配任意字符串。
- `remote_pattern` 的编写者需要注意正则转义。

### 5. 扫描目录推导 (`infer_scan_dir`)

从渲染后的远程路径正则中推导出实际要扫描的远程目录。

算法：

1. 找到路径中的最后一个 `/`，将其后的部分视为文件名范围。
2. 在目录部分中，从后向前找到第一个正则元字符（`.` `*` `+` `?` `(` `)` `[` `]` `{` `}` `|` `^` `$` `\`）。
3. 截取该元字符之前最后一个 `/` 之前的路径。
4. 如果没有正则元字符，直接返回整个目录部分。

这样设计是为了尽可能缩小扫描范围，避免从 `/` 开始递归扫描整个远程服务器。

#### 示例

| 渲染后的正则 | 推导的扫描目录 |
|---|---|
| `/data/WX/PA/20260210/DIR/FILE_.*` | `/data/WX/PA/20260210/DIR` |
| `/data/WX/PA/${SCAN_START_TIME,yyyyMMdd}/DIR/FILE.*` | `/data/WX/PA/20260210/DIR` |
| `/data/WX/*` | `/data/WX` |

### 6. 上下文日志 (`log_remote_context`)

远程文件扫描前，打印包含以下信息的日志，便于运维定位问题：

- 输入源类型（`ftp` / `sftp`）
- 远程主机和端口
- 远程用户
- SCAN_START_TIME 值
- 远程路径模板
- 渲染后的匹配正则
- 实际扫描的远程目录
- 本地下载目录
- 缓存保留天数

### 7. 远程目录预检查

在正式递归扫描前，先验证扫描目录是否存在且可访问：

- **FTP** (`ensure_ftp_scan_dir`)：
  - 记录当前工作目录。
  - 尝试 `cwd(scan_dir)`，成功则恢复原始目录。
  - 失败时返回包含 host、user、scan_dir、rendered_pattern 的排障错误。

- **SFTP** (`ensure_sftp_scan_dir`)：
  - 直接 `sftp.stat(Path::new(scan_dir))` 验证。
  - 失败时返回包含 host、user、scan_dir、rendered_pattern 的排障错误。

### 8. 远程文件扫描

#### FTP 递归扫描 (`list_ftp_recursive`)

通过 `ftp.nlst(path)` 获取当前目录下的条目列表，对每个条目：

- 拼接完整远程路径。
- 尝试 `cwd(完整路径)`：
  - 成功 → 是子目录，`cdup()` 回到上级，递归扫描该子目录。
  - 失败 → 是文件，加入文件列表。

使用 `cwd/错误` 来区分文件和目录，不依赖 FTP LIST 解析。

#### SFTP 递归扫描 (`list_sftp_recursive`)

通过 `sftp.readdir(path)` 获取目录内容，对每个条目：

- 过滤 `.` 和 `..`。
- 通过 `stat.perm` 的 `S_IFDIR` 位判断是否为目录：
  - 是目录 → 递归扫描。
  - 是文件 → 加入文件列表。

### 9. 正则匹配与排序

扫描完成后，对远程文件列表执行：

1. **正则过滤**：保留完整远程路径匹配 `remote_pattern` 渲染后正则的文件。
2. **字典序排序**：匹配到的文件按完整远程路径升序排序，确保导入顺序可预测。
3. **去重**：多个模板命中同一个远程文件时只下载一次。

多目录扫描策略：

- 某个扫描目录不存在或扫描失败时，打印警告并继续处理其他目录。
- 所有扫描目录都失败时，整体报错。
- 至少一个目录扫描成功但总匹配数为 0 时，整体报错并输出每个目录的扫描统计。

如果匹配数为 0，返回包含以下信息的详细排障错误：

```
远程目录扫描完成，但没有文件匹配正则。
source.type: sftp
scan_dir: /home/xxx/20260210/DIR
matched_regex: /home/xxx/20260210/DIR/FILE_20260210120000.*.csv
扫描到文件数: 12
匹配文件数: 0
请检查：
1. SCAN_START_TIME 日期是否正确
2. 文件时间、文件名前缀是否和 source.remote_pattern 一致
3. remote_pattern 是否写错
```

### 10. 远程文件下载

匹配到的文件下载到本地缓存目录。下载策略：

- `download_parallel = 1` 时串行下载，保持原行为。
- `download_parallel > 1` 时启用并行下载，每个 worker 独立建立 FTP/SFTP 连接。
- 并行下载中单个文件失败仍按 `download_retry` 重试。
- 并行下载完成后如果存在失败文件，整体报错并列出失败详情。

#### 临时文件

- 下载目标先写入 `<最终文件名>.part`。
- **下载成功** → `rename` 为最终文件名。
- **下载失败** → 清理 `.part` 文件，返回错误。

这种设计可以避免下载中断留下一个看起来完整的伪文件。

#### FTP 下载 (`download_ftp_file`)

```rust
let mut remote = ftp.retr_as_stream(remote_file)?;
let mut local = File::create(partial_file)?;
io::copy(&mut remote, &mut local)?;
ftp.finalize_retr_stream(remote)?;
fs::rename(partial_file, local_file)?;
```

- 使用 `retr_as_stream` 获取远程文件读取流。
- 使用 `io::copy` 将流式数据写入本地文件。
- 下载完成后必须调用 `finalize_retr_stream` 完成传输。
- 不一次性将整个文件读入内存。

#### SFTP 下载 (`download_sftp_file`)

```rust
let mut remote = sftp.open(Path::new(remote_file))?;
let mut local = File::create(partial_file)?;
io::copy(&mut remote, &mut local)?;
fs::rename(partial_file, local_file)?;
```

- 使用 `sftp.open` 获取远程文件读取句柄。
- 使用 `io::copy` 流式写入本地文件。
- SFTP 的 `open` 自动进入二进制读取模式。

#### 重复文件

如果本地缓存目录已存在同名的最终文件，会输出警告：

```
本地下载文件已存在，将覆盖: ./downloads/xxx.csv
```

不同远程目录下的同名文件会互相覆盖（当前设计假定不会出现这种情况）。

### 11. 本地缓存清理 (`cleanup_download_dir`)

在每次远程文件解析开始前执行：

- 遍历 `download_dir` 目录下的所有**普通文件**。
- 检查文件的修改时间。
- 超过 `source.cache_retention_days` 天的文件被删除。
- 不删除子目录下的文件（只清理直接子级的普通文件）。
- 不删除远程服务器上的文件。
- `.part` 是普通文件，如果超过保留期限也会被清理。

---

## 配置字段说明

### `[source]`

```toml
[source]
type = "sftp"                # 输入源类型: ftp | sftp
remote_pattern = "..."       # 远程路径模板（支持 ${SCAN_START_TIME,format}）
download_dir = "./downloads" # 远程文件下载到本地的缓存目录
cache_retention_days = 7     # 本地缓存文件保留天数
connect_retry = 3            # 连接和登录失败重试次数
download_retry = 3           # 单文件下载失败重试次数
download_parallel = 1        # 并行下载 worker 数，默认 1
retry_interval_secs = 30     # 两次重试之间的等待时间，默认 30 秒
connect_timeout_secs = 30    # 连接超时时间，默认 30 秒
read_timeout_secs = 300      # 读取/写入超时时间，默认 300 秒

[source.connection]
host = "127.0.0.1"
port = 22
username = "user"
password = "pass"
```

`scan_start_time` 不写入配置文件，远程模式必须通过 CLI 参数 `--scan-start-time "yyyy-MM-dd HH:mm:ss"` 传入。

---

## 错误处理策略

### 扫描阶段
- 单个目录不可访问或扫描递归失败 → 打印警告并继续其他目录。
- 所有目录都不可访问或扫描失败 → 返回包含每个目录失败原因的错误。
- 扫描完成但匹配数为 0 → 返回包含扫描目录、扫描文件数、匹配正则和 scan_start_time 的错误。

### 下载阶段
- 连接失败 → 返回带 host/user 上下文的连接错误。
- 登录失败 → 返回带 host/user 上下文的认证错误。
- 连接或登录失败 → 按 `connect_retry` 重试，每次失败打印日志；两次重试之间等待 `retry_interval_secs`。
- 单文件下载失败 → 清理 `.part` 临时文件，向上传播错误。
- 单文件下载失败 → 按 `download_retry` 重试，每次失败打印日志；两次重试之间等待 `retry_interval_secs`。
- `download_parallel > 1` 时启用多 worker 并行下载，每个 worker 独立建立连接。
- 流式 `io::copy` 失败 → 返回带远程路径和本地路径的错误信息。

### 导入阶段（`src/main.rs`）
- 导入阶段不在这里处理，由 `main.rs` 的多文件导入循环负责：
  - 单个文件失败记录失败文件并 `continue`。
  - 最终汇总失败列表。
  - 存在失败时返回总体错误。

---

## 本地文件兼容

当 CLI 传入 `--input` 时，整个远程连接和下载流程会跳过，直接返回本地文件或目录中的文件路径。`source.toml` 不支持 `type = "local"`；本地模式只通过 `--input` 选择。

---

## 已知限制

1. **FTP 被动模式**：`passive=false` 配置当前会被忽略并记录警告，因为 `suppaftp` 同步 API 默认走被动模式，没有提供 `set_passive(false)` 的方法。
2. **文件名冲突**：不同远程目录下同名文件会互相覆盖；当前业务约束要求远程文件名不重复。
3. **SFTP 认证**：当前仅支持用户名密码认证，不支持密钥认证。
4. **最大递归深度**：当前没有限制递归深度，配置不当可能导致扫描范围过大的远程目录树。
5. **点号语义**：`remote_pattern` 作为正则使用，`V3.3.0` 中的 `.` 会匹配任意字符。如果要求精确匹配点号，需要在配置中使用 `V3\\.3\\.0`。
