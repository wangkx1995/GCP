# Agent Output FTP/SFTP Transfer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Agent 采集成功后，将 `output/` 中每个二级唯一目录作为独立数据包，通过 FTP 或 SFTP 推送到 Core 文件服务，并在每个数据包完成后创建 `_SUCCESS`。

**Architecture:** 在 Agent 内新增独立的输出传输模块，不修改现有远程输入下载模块。传输模块扫描 `output/<一级分类目录>/<二级唯一数据包目录>/`，将每个数据包映射到 `remote_prefix/<一级目录>/<二级目录>/`，逐文件使用临时名上传后原子替换，最后创建 `_SUCCESS`。任务只有在解析和全部数据包上传均成功后才报告 `SUCCEEDED`；上传失败则任务报告 `FAILED`。

**Tech Stack:** Rust 2021、Tokio、Serde/TOML、`ssh2`、`suppaftp`、`walkdir`、`anyhow`、`tempfile`。

## Global Constraints

- Agent 主动连接 Core 文件服务并推送文件。
- 每个 Agent 只配置一个输出目标，协议为 `ftp` 或 `sftp`。
- 配置位于 `agent.toml` 的 `[transfer]` 区块，修改后重启 Agent 生效。
- 用户名和密码允许直接写入 TOML；任何日志和错误上下文不得输出密码。
- FTP 使用被动模式；允许普通 FTP 明文传输。
- SFTP 首版只支持用户名和密码认证。
- 本地目录结构固定为 `output/<一级分类目录>/<二级唯一数据包目录>/*`。
- 一级分类目录允许在不同任务间重复，例如 `tpd_eutr_prb_q_2026061714`。
- 二级数据包目录在业务上唯一，例如 `LTE_PM_1604007_202606171445`。
- 远程路径固定为 `remote_prefix/<一级分类目录>/<二级唯一数据包目录>/...`，不额外拼接 `task_id`。
- 每个二级目录是一个独立上传事务和 Core 可见单元。
- 同名远程文件使用临时文件上传后原子覆盖。
- 上传一个二级目录前，只删除该二级目录中的旧 `_SUCCESS`，不得删除一级目录下其他数据包的标记。
- 一个二级目录中的全部普通文件成功后，才在该目录创建空 `_SUCCESS`。
- Core 必须仅处理存在 `_SUCCESS` 的二级数据包目录。
- `output/` 为空，或不存在任何有效二级数据包目录时，任务成功但不连接远程服务器。
- 仅上传普通文件；软链接、设备文件及其他特殊文件忽略且不得跟随。
- 一个任务内部串行上传；现有不同任务并发模型保持不变。
- 默认上传尝试总次数为 3 次，每次失败后等待 5 秒；均可配置。
- 重试耗尽后整个采集任务失败，本地输出保留。
- 上传成功的任务输出默认保留 7 天；失败任务输出不自动删除。
- Agent 启动时执行一次清理，之后每 24 小时执行一次。
- 传输已启用但配置无效时，Agent 启动失败并指出具体配置字段。
- 远程路径必须拒绝绝对路径、`.`、`..`、路径分隔符注入及逃逸 `remote_prefix` 的名称。
- 不修改 `crates/remote-file-source` 的输入下载职责。
- 不新增独立上传进程或服务。
- 所有新增生产代码严格执行测试先行：先观察测试按预期失败，再编写最小实现。

---

## File Structure

- Modify `Cargo.toml`: 为主 crate 增加 `ssh2` 和 `suppaftp` 直接依赖。
- Modify `agent.toml`: 增加带默认示例的 `[transfer]` 配置。
- Modify `src/agent/mod.rs`: 导出 `transfer` 模块。
- Create `src/agent/transfer/mod.rs`: 数据包发现、路径映射、重试、上传编排和公开接口。
- Create `src/agent/transfer/config.rs`: `[transfer]` 的反序列化模型、默认值和启动校验。
- Create `src/agent/transfer/backend.rs`: 定义可测试的远程文件操作边界和协议分发。
- Create `src/agent/transfer/ftp.rs`: FTP 连接、递归建目录、临时上传、重命名、删除标记和创建标记。
- Create `src/agent/transfer/sftp.rs`: SFTP 对应实现。
- Modify `src/bin/agent.rs`: 解析并校验传输配置，传入 Agent 服务。
- Modify `src/agent/server.rs`: 持有共享传输器，启动清理循环，并传给每个任务执行器。
- Modify `src/agent/runner.rs`: 解析成功后先上传，再更新最终任务状态和上报 Core。
- Modify `src/agent/store.rs`: 记录上传成功时间，并清理超过保留期的成功任务目录。
- Modify `docs/core-agent-test-guide.md`: 增加 FTP/SFTP 输出推送部署与验收步骤。

---

### Task 1: Add And Validate Transfer Configuration

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `agent.toml`
- Create: `src/agent/transfer/config.rs`
- Create: `src/agent/transfer/mod.rs`
- Modify: `src/agent/mod.rs`
- Modify: `src/bin/agent.rs`

**Interfaces:**
- Produces: `TransferConfig`
- Produces: `TransferProtocol::{Ftp, Sftp}`
- Produces: `TransferConfig::validate(&self) -> anyhow::Result<()>`
- Produces: `AgentConfig.transfer: TransferConfig`
- Consumed later by: `OutputTransfer::new(config)` and cleanup scheduling.

- [ ] **Step 1: Write failing configuration default test**

Create `src/agent/transfer/config.rs` with serde model shells and this test:

```rust
use anyhow::{bail, Result};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferProtocol {
    Ftp,
    Sftp,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TransferConfig {
    #[serde(default)]
    pub enabled: bool,
    pub protocol: Option<TransferProtocol>,
    #[serde(default)]
    pub host: String,
    pub port: Option<u16>,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub remote_prefix: String,
    #[serde(default = "default_retry_count")]
    pub retry_count: usize,
    #[serde(default = "default_retry_interval_seconds")]
    pub retry_interval_seconds: u64,
    #[serde(default = "default_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
    #[serde(default = "default_operation_timeout_seconds")]
    pub operation_timeout_seconds: u64,
    #[serde(default = "default_success_retention_days")]
    pub success_retention_days: u64,
    #[serde(default = "default_cleanup_interval_hours")]
    pub cleanup_interval_hours: u64,
    #[serde(default = "default_ftp_passive")]
    pub ftp_passive: bool,
}

fn default_retry_count() -> usize { 3 }
fn default_retry_interval_seconds() -> u64 { 5 }
fn default_connect_timeout_seconds() -> u64 { 10 }
fn default_operation_timeout_seconds() -> u64 { 60 }
fn default_success_retention_days() -> u64 { 7 }
fn default_cleanup_interval_hours() -> u64 { 24 }
fn default_ftp_passive() -> bool { true }

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            protocol: None,
            host: String::new(),
            port: None,
            username: String::new(),
            password: String::new(),
            remote_prefix: String::new(),
            retry_count: default_retry_count(),
            retry_interval_seconds: default_retry_interval_seconds(),
            connect_timeout_seconds: default_connect_timeout_seconds(),
            operation_timeout_seconds: default_operation_timeout_seconds(),
            success_retention_days: default_success_retention_days(),
            cleanup_interval_hours: default_cleanup_interval_hours(),
            ftp_passive: default_ftp_passive(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_config_is_disabled_with_expected_defaults_when_section_is_absent() {
        #[derive(Deserialize)]
        struct Root {
            #[serde(default)]
            transfer: TransferConfig,
        }

        let root: Root = toml::from_str("").unwrap();

        assert!(!root.transfer.enabled);
        assert_eq!(root.transfer.retry_count, 3);
        assert_eq!(root.transfer.retry_interval_seconds, 5);
        assert_eq!(root.transfer.connect_timeout_seconds, 10);
        assert_eq!(root.transfer.operation_timeout_seconds, 60);
        assert_eq!(root.transfer.success_retention_days, 7);
        assert_eq!(root.transfer.cleanup_interval_hours, 24);
        assert!(root.transfer.ftp_passive);
    }
}
```

- [ ] **Step 2: Run the test and verify RED**

Run: `cargo test agent::transfer::config::tests::transfer_config_is_disabled_with_expected_defaults_when_section_is_absent -v`

Expected: compilation fails because `agent::transfer` is not exported.

- [ ] **Step 3: Export the module and make the default test pass**

Create `src/agent/transfer/mod.rs`:

```rust
pub mod config;
```

Add to `src/agent/mod.rs`:

```rust
pub mod transfer;
```

Run the focused test again.

Expected: PASS.

- [ ] **Step 4: Write failing validation tests**

Add tests covering enabled configuration:

```rust
#[test]
fn enabled_transfer_requires_connection_and_remote_prefix() {
    let config: TransferConfig = toml::from_str(
        r#"
        enabled = true
        protocol = "sftp"
        host = ""
        port = 22
        username = "agent"
        password = "secret"
        remote_prefix = "/core/uploads"
        "#,
    ).unwrap();

    let error = config.validate().unwrap_err();
    assert!(error.to_string().contains("transfer.host"));
}

#[test]
fn enabled_transfer_rejects_zero_retry_and_timeout_values() {
    let mut config: TransferConfig = toml::from_str(
        r#"
        enabled = true
        protocol = "ftp"
        host = "127.0.0.1"
        port = 21
        username = "agent"
        password = "secret"
        remote_prefix = "/core/uploads"
        "#,
    ).unwrap();
    config.retry_count = 0;

    assert!(config.validate().unwrap_err().to_string().contains("retry_count"));
}

#[test]
fn disabled_transfer_ignores_empty_connection_fields() {
    TransferConfig::default().validate().unwrap();
}
```

- [ ] **Step 5: Run validation tests and verify RED**

Run: `cargo test agent::transfer::config::tests -v`

Expected: compilation fails because `TransferConfig::validate` does not exist.

- [ ] **Step 6: Implement minimal validation**

Implement `validate` with explicit field errors:

```rust
impl TransferConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.protocol.is_none() {
            bail!("transfer.protocol is required when transfer.enabled=true");
        }
        if self.host.trim().is_empty() {
            bail!("transfer.host must not be empty");
        }
        if self.port.unwrap_or(0) == 0 {
            bail!("transfer.port must be greater than 0");
        }
        if self.username.trim().is_empty() {
            bail!("transfer.username must not be empty");
        }
        if self.password.is_empty() {
            bail!("transfer.password must not be empty");
        }
        if self.remote_prefix.trim().is_empty() {
            bail!("transfer.remote_prefix must not be empty");
        }
        if self.retry_count == 0 {
            bail!("transfer.retry_count must be greater than 0");
        }
        if self.retry_interval_seconds == 0 {
            bail!("transfer.retry_interval_seconds must be greater than 0");
        }
        if self.connect_timeout_seconds == 0 {
            bail!("transfer.connect_timeout_seconds must be greater than 0");
        }
        if self.operation_timeout_seconds == 0 {
            bail!("transfer.operation_timeout_seconds must be greater than 0");
        }
        if self.cleanup_interval_hours == 0 {
            bail!("transfer.cleanup_interval_hours must be greater than 0");
        }
        Ok(())
    }

    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or(match self.protocol {
            Some(TransferProtocol::Ftp) => 21,
            Some(TransferProtocol::Sftp) | None => 22,
        })
    }
}
```

Run: `cargo test agent::transfer::config::tests -v`

Expected: PASS.

- [ ] **Step 7: Wire configuration into the Agent binary**

In `src/bin/agent.rs`:

```rust
use wy_gnb_pm_parser::agent::transfer::config::TransferConfig;

#[derive(Deserialize)]
struct AgentConfig {
    core: CoreConfig,
    agent: AgentSettings,
    #[serde(default)]
    transfer: TransferConfig,
}
```

Immediately after TOML parsing:

```rust
config.transfer.validate()?;
```

Pass `config.transfer` to `run_agent_server` after the existing heartbeat argument.

- [ ] **Step 8: Add direct protocol dependencies**

Add to root `Cargo.toml`:

```toml
ssh2 = { version = "0.9", features = ["vendored-openssl"] }
suppaftp = "6.0"
```

Run: `cargo check --bin agent`

Expected: compilation fails only because `run_agent_server` does not yet accept `TransferConfig`; Task 5 completes this interface.

- [ ] **Step 9: Add the example configuration**

Append to `agent.toml`:

```toml
[transfer]
enabled = false
protocol = "sftp"
host = "127.0.0.1"
port = 22
username = "agent"
password = "password"
remote_prefix = "/core/uploads"
retry_count = 3
retry_interval_seconds = 5
connect_timeout_seconds = 10
operation_timeout_seconds = 60
success_retention_days = 7
cleanup_interval_hours = 24
ftp_passive = true
```

- [ ] **Step 10: Commit configuration support**

```bash
git add Cargo.toml Cargo.lock agent.toml src/agent/mod.rs src/agent/transfer/mod.rs src/agent/transfer/config.rs src/bin/agent.rs
git commit -m "feat(agent): add output transfer configuration"
```

---

### Task 2: Discover Unique Second-Level Output Packages Safely

**Files:**
- Modify: `src/agent/transfer/mod.rs`

**Interfaces:**
- Produces: `OutputPackage { local_dir, remote_dir, files }`
- Produces: `discover_output_packages(output_dir, remote_prefix) -> Result<Vec<OutputPackage>>`
- Guarantees: only `output/<level1>/<level2>/` directories become packages.
- Guarantees: level 1 may repeat; level 2 is treated as the unique package identifier.

- [ ] **Step 1: Write failing discovery and mapping tests**

Add to `src/agent/transfer/mod.rs`:

```rust
use std::path::{Path, PathBuf};
use anyhow::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputFile {
    pub local_path: PathBuf,
    pub relative_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputPackage {
    pub local_dir: PathBuf,
    pub remote_dir: String,
    pub files: Vec<OutputFile>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn discovers_each_second_level_directory_as_independent_package() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("output");
        let package_a = output
            .join("tpd_eutr_prb_q_2026061714")
            .join("LTE_PM_1604007_202606171445");
        let package_b = output
            .join("tpd_eutr_prb_q_2026061714")
            .join("LTE_PM_1604008_202606171445");
        std::fs::create_dir_all(package_a.join("nested")).unwrap();
        std::fs::create_dir_all(&package_b).unwrap();
        std::fs::write(package_a.join("a.csv"), b"a").unwrap();
        std::fs::write(package_a.join("nested/meta.ini"), b"m").unwrap();
        std::fs::write(package_b.join("b.csv"), b"b").unwrap();

        let packages = discover_output_packages(&output, "/core/uploads").unwrap();

        assert_eq!(packages.len(), 2);
        assert_eq!(
            packages[0].remote_dir,
            "/core/uploads/tpd_eutr_prb_q_2026061714/LTE_PM_1604007_202606171445"
        );
        assert_eq!(
            packages[0].files.iter().map(|file| file.relative_path.clone()).collect::<Vec<_>>(),
            vec![PathBuf::from("a.csv"), PathBuf::from("nested/meta.ini")]
        );
    }

    #[test]
    fn ignores_files_directly_under_output_and_first_level_directories() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("output");
        std::fs::create_dir_all(output.join("level1")).unwrap();
        std::fs::write(output.join("orphan.txt"), b"x").unwrap();
        std::fs::write(output.join("level1/orphan.txt"), b"x").unwrap();

        let packages = discover_output_packages(&output, "/core/uploads").unwrap();

        assert!(packages.is_empty());
    }

    #[test]
    fn ignores_symlinks_in_package_tree() {
        #[cfg(unix)]
        {
            let dir = tempdir().unwrap();
            let output = dir.path().join("output");
            let package = output.join("level1/unique-package");
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(dir.path().join("outside.txt"), b"secret").unwrap();
            std::os::unix::fs::symlink(
                dir.path().join("outside.txt"),
                package.join("linked.txt"),
            ).unwrap();

            let packages = discover_output_packages(&output, "/core/uploads").unwrap();

            assert_eq!(packages.len(), 1);
            assert!(packages[0].files.is_empty());
        }
    }

    #[test]
    fn rejects_unsafe_directory_names() {
        assert!(safe_remote_component("..").is_err());
        assert!(safe_remote_component(".").is_err());
        assert!(safe_remote_component("a/b").is_err());
        assert!(safe_remote_component("a\\b").is_err());
        assert!(safe_remote_component("LTE_PM_1604007_202606171445").is_ok());
    }
}
```

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test agent::transfer::tests -v`

Expected: compilation fails because discovery functions do not exist.

- [ ] **Step 3: Implement safe two-level discovery**

Implement:

```rust
use walkdir::WalkDir;

fn safe_remote_component(value: &str) -> Result<&str> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        anyhow::bail!("unsafe output directory name: {value:?}");
    }
    Ok(value)
}

pub fn discover_output_packages(output_dir: &Path, remote_prefix: &str) -> Result<Vec<OutputPackage>> {
    if !output_dir.exists() {
        return Ok(Vec::new());
    }
    let prefix = remote_prefix.trim_end_matches('/');
    let mut packages = Vec::new();
    for level1 in std::fs::read_dir(output_dir)? {
        let level1 = level1?;
        if !level1.file_type()?.is_dir() {
            continue;
        }
        let level1_name = level1.file_name().to_string_lossy().to_string();
        safe_remote_component(&level1_name)?;
        for level2 in std::fs::read_dir(level1.path())? {
            let level2 = level2?;
            if !level2.file_type()?.is_dir() {
                continue;
            }
            let level2_name = level2.file_name().to_string_lossy().to_string();
            safe_remote_component(&level2_name)?;
            let local_dir = level2.path();
            let mut files = WalkDir::new(&local_dir)
                .follow_links(false)
                .min_depth(1)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_file())
                .map(|entry| {
                    let local_path = entry.path().to_path_buf();
                    let relative_path = local_path.strip_prefix(&local_dir)?.to_path_buf();
                    Ok(OutputFile { local_path, relative_path })
                })
                .collect::<Result<Vec<_>>>()?;
            files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
            packages.push(OutputPackage {
                local_dir,
                remote_dir: format!("{prefix}/{level1_name}/{level2_name}"),
                files,
            });
        }
    }
    packages.sort_by(|left, right| left.remote_dir.cmp(&right.remote_dir));
    Ok(packages)
}
```

Run: `cargo test agent::transfer::tests -v`

Expected: PASS.

- [ ] **Step 4: Commit package discovery**

```bash
git add src/agent/transfer/mod.rs
git commit -m "feat(agent): discover second-level output packages"
```

---

### Task 3: Define A Testable Remote Upload Boundary

**Files:**
- Create: `src/agent/transfer/backend.rs`
- Modify: `src/agent/transfer/mod.rs`

**Interfaces:**
- Produces trait: `TransferBackend`
- Produces: `upload_package_with_backend(backend, package) -> Result<()>`
- Ordering contract: remove old marker, create directories, upload each file to `.part`, rename, create marker.

- [ ] **Step 1: Write failing orchestration test with a recording backend**

Create `src/agent/transfer/backend.rs`:

```rust
use std::path::Path;
use anyhow::Result;

pub trait TransferBackend {
    fn ensure_dir(&mut self, remote_dir: &str) -> Result<()>;
    fn remove_file_if_exists(&mut self, remote_path: &str) -> Result<()>;
    fn upload_file(&mut self, local_path: &Path, remote_path: &str) -> Result<()>;
    fn rename_replace(&mut self, from: &str, to: &str) -> Result<()>;
    fn create_empty_file(&mut self, remote_path: &str) -> Result<()>;
}
```

Add to `src/agent/transfer/mod.rs` tests:

```rust
#[derive(Default)]
struct RecordingBackend {
    operations: Vec<String>,
}

impl backend::TransferBackend for RecordingBackend {
    fn ensure_dir(&mut self, path: &str) -> Result<()> {
        self.operations.push(format!("mkdir:{path}"));
        Ok(())
    }
    fn remove_file_if_exists(&mut self, path: &str) -> Result<()> {
        self.operations.push(format!("remove:{path}"));
        Ok(())
    }
    fn upload_file(&mut self, local: &Path, remote: &str) -> Result<()> {
        self.operations.push(format!("upload:{}:{remote}", local.display()));
        Ok(())
    }
    fn rename_replace(&mut self, from: &str, to: &str) -> Result<()> {
        self.operations.push(format!("rename:{from}:{to}"));
        Ok(())
    }
    fn create_empty_file(&mut self, path: &str) -> Result<()> {
        self.operations.push(format!("touch:{path}"));
        Ok(())
    }
}

#[test]
fn uploads_package_atomically_and_creates_success_marker_last() {
    let dir = tempdir().unwrap();
    let package_dir = dir.path().join("package");
    std::fs::create_dir_all(package_dir.join("nested")).unwrap();
    std::fs::write(package_dir.join("a.csv"), b"a").unwrap();
    std::fs::write(package_dir.join("nested/a.ini"), b"i").unwrap();
    let package = OutputPackage {
        local_dir: package_dir.clone(),
        remote_dir: "/core/uploads/level1/package".to_string(),
        files: vec![
            OutputFile { local_path: package_dir.join("a.csv"), relative_path: PathBuf::from("a.csv") },
            OutputFile { local_path: package_dir.join("nested/a.ini"), relative_path: PathBuf::from("nested/a.ini") },
        ],
    };
    let mut backend = RecordingBackend::default();

    upload_package_with_backend(&mut backend, &package).unwrap();

    assert_eq!(backend.operations.first().unwrap(), "remove:/core/uploads/level1/package/_SUCCESS");
    assert_eq!(backend.operations.last().unwrap(), "touch:/core/uploads/level1/package/_SUCCESS");
    assert!(backend.operations.contains(&"upload:/tmp/ignored:/core/uploads/level1/package/a.csv.part".to_string()) == false);
    assert!(backend.operations.iter().any(|op| op.ends_with(":/core/uploads/level1/package/a.csv.part")));
    assert!(backend.operations.iter().any(|op| op == "rename:/core/uploads/level1/package/a.csv.part:/core/uploads/level1/package/a.csv"));
    assert!(backend.operations.iter().any(|op| op == "mkdir:/core/uploads/level1/package/nested"));
}
```

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test agent::transfer::tests::uploads_package_atomically_and_creates_success_marker_last -v`

Expected: compilation fails because `upload_package_with_backend` does not exist.

- [ ] **Step 3: Implement minimal upload orchestration**

In `src/agent/transfer/mod.rs`:

```rust
pub mod backend;

fn join_remote(base: &str, relative: &Path) -> Result<String> {
    let mut remote = base.trim_end_matches('/').to_string();
    for component in relative.components() {
        let std::path::Component::Normal(value) = component else {
            anyhow::bail!("unsafe relative output path: {}", relative.display());
        };
        let value = value.to_string_lossy();
        safe_remote_component(&value)?;
        remote.push('/');
        remote.push_str(&value);
    }
    Ok(remote)
}

pub fn upload_package_with_backend(
    backend: &mut dyn backend::TransferBackend,
    package: &OutputPackage,
) -> Result<()> {
    let marker = format!("{}/_SUCCESS", package.remote_dir);
    backend.remove_file_if_exists(&marker)?;
    backend.ensure_dir(&package.remote_dir)?;
    for file in &package.files {
        let final_path = join_remote(&package.remote_dir, &file.relative_path)?;
        let parent = final_path.rsplit_once('/').map(|(parent, _)| parent).unwrap_or(&package.remote_dir);
        backend.ensure_dir(parent)?;
        let part_path = format!("{final_path}.part");
        backend.remove_file_if_exists(&part_path)?;
        backend.upload_file(&file.local_path, &part_path)?;
        backend.rename_replace(&part_path, &final_path)?;
    }
    backend.create_empty_file(&marker)?;
    Ok(())
}
```

Run focused tests.

Expected: PASS.

- [ ] **Step 4: Write marker failure test**

Add a backend that fails on `create_empty_file` and assert `upload_package_with_backend` returns an error. This proves marker creation is part of task success, not best effort.

- [ ] **Step 5: Commit upload orchestration**

```bash
git add src/agent/transfer/backend.rs src/agent/transfer/mod.rs
git commit -m "feat(agent): add atomic output package upload flow"
```

---

### Task 4: Implement FTP And SFTP Backends

**Files:**
- Create: `src/agent/transfer/ftp.rs`
- Create: `src/agent/transfer/sftp.rs`
- Modify: `src/agent/transfer/backend.rs`
- Modify: `src/agent/transfer/mod.rs`

**Interfaces:**
- Produces: `connect_backend(config: &TransferConfig) -> Result<Box<dyn TransferBackend + Send>>`
- FTP: passive connection, recursive directory creation, `STOR`, rename and marker upload.
- SFTP: password authentication, recursive directory creation, create/write, unlink and rename.

- [ ] **Step 1: Write failing protocol dispatch test**

In `backend.rs`, add a small protocol-to-builder selection helper test that does not open a network connection:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendKind { Ftp, Sftp }

fn backend_kind(config: &TransferConfig) -> Result<BackendKind> {
    match config.protocol {
        Some(TransferProtocol::Ftp) => Ok(BackendKind::Ftp),
        Some(TransferProtocol::Sftp) => Ok(BackendKind::Sftp),
        None => anyhow::bail!("transfer.protocol is required"),
    }
}

#[test]
fn selects_backend_from_transfer_protocol() {
    let mut config = TransferConfig::default();
    config.protocol = Some(TransferProtocol::Ftp);
    assert_eq!(backend_kind(&config).unwrap(), BackendKind::Ftp);
    config.protocol = Some(TransferProtocol::Sftp);
    assert_eq!(backend_kind(&config).unwrap(), BackendKind::Sftp);
}
```

Run and verify the test fails before adding the helper.

- [ ] **Step 2: Implement FTP backend**

`src/agent/transfer/ftp.rs` must:

```rust
pub struct FtpTransferBackend {
    stream: suppaftp::FtpStream,
}
```

Connection requirements:

```rust
let address = format!("{}:{}", config.host, config.effective_port());
let socket = address.to_socket_addrs()?.next().context("FTP address resolved to no socket")?;
let mut stream = FtpStream::connect_timeout(socket, Duration::from_secs(config.connect_timeout_seconds))?;
stream.get_ref().set_read_timeout(Some(Duration::from_secs(config.operation_timeout_seconds)))?;
stream.get_ref().set_write_timeout(Some(Duration::from_secs(config.operation_timeout_seconds)))?;
stream.login(&config.username, &config.password)
    .with_context(|| format!("failed to login FTP user={}", config.username))?;
stream.transfer_type(suppaftp::types::FileType::Binary)?;
```

Implementation requirements:

- `ensure_dir` walks slash-separated components and ignores “already exists” errors only after confirming `cwd` succeeds.
- `upload_file` opens the local file and calls `put_file`/equivalent streaming API.
- `remove_file_if_exists` ignores only FTP “file unavailable/not found” responses.
- `rename_replace` removes the destination if present, then performs FTP rename.
- `create_empty_file` uploads a zero-byte cursor.
- Do not include `config.password` in any error message or tracing field.
- `ftp_passive=false` is rejected by config validation in this version because only passive mode is supported.

- [ ] **Step 3: Implement SFTP backend**

`src/agent/transfer/sftp.rs` must own both session and SFTP handles so the SSH session remains alive:

```rust
pub struct SftpTransferBackend {
    _session: ssh2::Session,
    sftp: ssh2::Sftp,
}
```

Connection requirements mirror the existing input client but use `TransferConfig`:

```rust
let tcp = TcpStream::connect_timeout(&socket, Duration::from_secs(config.connect_timeout_seconds))?;
tcp.set_read_timeout(Some(Duration::from_secs(config.operation_timeout_seconds)))?;
tcp.set_write_timeout(Some(Duration::from_secs(config.operation_timeout_seconds)))?;
let mut session = Session::new()?;
session.set_tcp_stream(tcp);
session.handshake()?;
session.userauth_password(&config.username, &config.password)
    .with_context(|| format!("failed to login SFTP user={}", config.username))?;
let sftp = session.sftp()?;
```

Implementation requirements:

- `ensure_dir` walks from `/` for absolute remote prefixes and calls `mkdir(..., 0o755)`.
- Existing directories are accepted only after `stat` confirms they are directories.
- `upload_file` uses `sftp.create` plus `std::io::copy`.
- `remove_file_if_exists` ignores only SFTP “no such file” errors.
- `rename_replace` removes destination first, then uses `rename`.
- `create_empty_file` creates and closes a zero-byte file.

- [ ] **Step 4: Add protocol factory**

In `backend.rs`:

```rust
pub fn connect_backend(config: &TransferConfig) -> Result<Box<dyn TransferBackend + Send>> {
    match backend_kind(config)? {
        BackendKind::Ftp => Ok(Box::new(crate::agent::transfer::ftp::connect(config)?)),
        BackendKind::Sftp => Ok(Box::new(crate::agent::transfer::sftp::connect(config)?)),
    }
}
```

Export modules from `transfer/mod.rs`:

```rust
mod ftp;
mod sftp;
```

- [ ] **Step 5: Compile and run unit tests**

Run:

```bash
cargo test agent::transfer -v
cargo check --bin agent
```

Expected: all transfer unit tests pass; Agent compiles except for the pending `run_agent_server` signature change from Task 5.

- [ ] **Step 6: Commit protocol backends**

```bash
git add src/agent/transfer/backend.rs src/agent/transfer/ftp.rs src/agent/transfer/sftp.rs src/agent/transfer/mod.rs
git commit -m "feat(agent): implement ftp and sftp output backends"
```

---

### Task 5: Add Retry And Integrate Upload Into Task Completion

**Files:**
- Modify: `src/agent/transfer/mod.rs`
- Modify: `src/agent/runner.rs`
- Modify: `src/agent/server.rs`
- Modify: `src/bin/agent.rs`

**Interfaces:**
- Produces: `OutputTransfer::new(config: TransferConfig)`
- Produces: `OutputTransfer::upload_output(&self, output_dir: &Path) -> Result<TransferSummary>`
- Produces: `TransferSummary { package_count, file_count }`
- Changes: `AgentRunner` owns `OutputTransfer`.
- Changes: task state becomes `SUCCEEDED` only after output upload succeeds.

- [ ] **Step 1: Write failing retry tests using an injected backend factory**

Design `OutputTransfer` with an internal factory closure so tests can return recording/failing backends without network access:

```rust
type BackendFactory = Arc<dyn Fn() -> Result<Box<dyn TransferBackend + Send>> + Send + Sync>;

#[derive(Clone)]
pub struct OutputTransfer {
    config: TransferConfig,
    backend_factory: BackendFactory,
}
```

Add tests:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

struct FailingUploadBackend {
    attempts: Arc<AtomicUsize>,
    failures_before_success: usize,
}

impl TransferBackend for FailingUploadBackend {
    fn ensure_dir(&mut self, _path: &str) -> Result<()> { Ok(()) }
    fn remove_file_if_exists(&mut self, _path: &str) -> Result<()> { Ok(()) }
    fn upload_file(&mut self, _local: &Path, _remote: &str) -> Result<()> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        if attempt <= self.failures_before_success {
            anyhow::bail!("injected upload failure {attempt}");
        }
        Ok(())
    }
    fn rename_replace(&mut self, _from: &str, _to: &str) -> Result<()> { Ok(()) }
    fn create_empty_file(&mut self, _path: &str) -> Result<()> { Ok(()) }
}

fn retry_test_output() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let output = dir.path().join("output");
    let package = output.join("level1/unique-package");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("data.csv"), b"data").unwrap();
    (dir, output)
}

#[test]
fn retries_failed_package_upload_up_to_configured_attempts() {
    let (_dir, output) = retry_test_output();
    let attempts = Arc::new(AtomicUsize::new(0));
    let factory_attempts = Arc::clone(&attempts);
    let transfer = OutputTransfer::new_for_test(
        TransferConfig {
            enabled: true,
            remote_prefix: "/core/uploads".to_string(),
            retry_count: 3,
            retry_interval_seconds: 0,
            ..TransferConfig::default()
        },
        Arc::new(move || {
            Ok(Box::new(FailingUploadBackend {
                attempts: Arc::clone(&factory_attempts),
                failures_before_success: 2,
            }))
        }),
    );

    let summary = transfer.upload_output(&output).unwrap();

    assert_eq!(summary.package_count, 1);
    assert_eq!(summary.file_count, 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[test]
fn returns_error_after_retry_count_is_exhausted() {
    let (_dir, output) = retry_test_output();
    let attempts = Arc::new(AtomicUsize::new(0));
    let factory_attempts = Arc::clone(&attempts);
    let transfer = OutputTransfer::new_for_test(
        TransferConfig {
            enabled: true,
            remote_prefix: "/core/uploads".to_string(),
            retry_count: 3,
            retry_interval_seconds: 0,
            ..TransferConfig::default()
        },
        Arc::new(move || {
            Ok(Box::new(FailingUploadBackend {
                attempts: Arc::clone(&factory_attempts),
                failures_before_success: usize::MAX,
            }))
        }),
    );

    let error = transfer.upload_output(&output).unwrap_err();

    assert!(error.to_string().contains("unique-package"));
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[test]
fn empty_output_returns_success_without_creating_backend() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("output");
    std::fs::create_dir_all(&output).unwrap();
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&factory_calls);
    let transfer = OutputTransfer::new_for_test(
        TransferConfig {
            enabled: true,
            remote_prefix: "/core/uploads".to_string(),
            ..TransferConfig::default()
        },
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("backend must not be created for empty output")
        }),
    );

    let summary = transfer.upload_output(&output).unwrap();

    assert_eq!(summary, TransferSummary { package_count: 0, file_count: 0 });
    assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
}
```

Add this test-only constructor inside `impl OutputTransfer`:

```rust
#[cfg(test)]
fn new_for_test(config: TransferConfig, backend_factory: BackendFactory) -> Self {
    Self { config, backend_factory }
}
```

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test agent::transfer::tests -v`

Expected: compilation fails because `OutputTransfer` and `TransferSummary` do not exist.

- [ ] **Step 3: Implement retrying output transfer**

Required behavior:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferSummary {
    pub package_count: usize,
    pub file_count: usize,
}

impl OutputTransfer {
    pub fn new(config: TransferConfig) -> Self {
        let factory_config = config.clone();
        Self {
            config,
            backend_factory: Arc::new(move || backend::connect_backend(&factory_config)),
        }
    }

    pub fn upload_output(&self, output_dir: &Path) -> Result<TransferSummary> {
        if !self.config.enabled {
            return Ok(TransferSummary { package_count: 0, file_count: 0 });
        }
        let packages = discover_output_packages(output_dir, &self.config.remote_prefix)?;
        if packages.is_empty() {
            return Ok(TransferSummary { package_count: 0, file_count: 0 });
        }
        let file_count = packages.iter().map(|package| package.files.len()).sum();
        for package in &packages {
            self.upload_package_with_retry(package)?;
        }
        Ok(TransferSummary { package_count: packages.len(), file_count })
    }
}
```

Each retry must create a new connection through `backend_factory`, log protocol/host/port/remote package path/attempt, and never log the password.

- [ ] **Step 4: Write a failing runner status-order test**

Extract a pure status decision helper before changing the runner branch:

```rust
fn terminal_status(parse_succeeded: bool, transfer_result: &Result<TransferSummary>) -> TaskStatus {
    if parse_succeeded && transfer_result.is_ok() {
        TaskStatus::Succeeded
    } else {
        TaskStatus::Failed
    }
}
```

Write these tests first:

```rust
#[test]
fn parse_and_upload_success_produce_succeeded_status() {
    let transfer = Ok(TransferSummary { package_count: 1, file_count: 4 });
    assert_eq!(terminal_status(true, &transfer), TaskStatus::Succeeded);
}

#[test]
fn upload_failure_changes_parse_success_to_failed_status() {
    let transfer = Err(anyhow::anyhow!("upload failed"));
    assert_eq!(terminal_status(true, &transfer), TaskStatus::Failed);
}

#[test]
fn parse_failure_remains_failed_without_upload() {
    let transfer = Ok(TransferSummary { package_count: 0, file_count: 0 });
    assert_eq!(terminal_status(false, &transfer), TaskStatus::Failed);
}
```

Run: `cargo test agent::runner::tests -v`

Expected: compilation fails because `terminal_status` does not exist.

Implement the helper exactly as shown, then use it in the runner flow. The integrated branch must prove:

- parse success + upload success => local state and Core report are `SUCCEEDED`;
- parse success + upload failure => local state and Core report are `FAILED`;
- parse failure => upload is not called and status is `FAILED`.

Do not set `TaskStatus::Succeeded` at the current `run_parse_job` success branch before upload.

- [ ] **Step 5: Integrate `OutputTransfer` into `AgentRunner`**

Change the runner structure:

```rust
#[derive(Clone)]
pub struct AgentRunner {
    pub agent_id: String,
    pub tcp_tx: mpsc::Sender<InternalMessage>,
    pub output_transfer: OutputTransfer,
}
```

In the parse success branch:

```rust
match self.output_transfer.upload_output(&output_dir) {
    Ok(summary) => {
        tracing::info!(
            "[agent] output transfer completed: packages={} files={}",
            summary.package_count,
            summary.file_count
        );
        let csv_rows = read_result_rows(&output_dir).unwrap_or_else(|error| {
            tracing::error!("[agent] read result.csv failed: {error:#}");
            Vec::new()
        });
        store.mark_task_succeeded(&task.task_id)?;
        (TaskStatus::Succeeded, csv_rows)
    }
    Err(error) => {
        tracing::error!("[agent] output transfer failed: {error:#}");
        store.update_task_state(&task.task_id, TaskStatus::Failed)?;
        (TaskStatus::Failed, Vec::new())
    }
}
```

Use a dedicated `mark_task_succeeded` method added in Task 6 so cleanup has a reliable timestamp.

- [ ] **Step 6: Pass transfer configuration through Agent startup**

Change `run_agent_server` to accept:

```rust
transfer_config: TransferConfig,
```

Construct one reusable transfer object:

```rust
let output_transfer = OutputTransfer::new(transfer_config.clone());
```

Pass it into `AgentRunner`. Keep current task-level `tokio::spawn` behavior; each task runs its own serial package upload flow.

- [ ] **Step 7: Run focused and full tests**

Run:

```bash
cargo test agent::transfer -v
cargo test agent::runner -v
cargo test
```

Expected: all tests pass.

- [ ] **Step 8: Commit task integration**

```bash
git add src/agent/transfer/mod.rs src/agent/runner.rs src/agent/server.rs src/bin/agent.rs
git commit -m "feat(agent): upload output before reporting task success"
```

---

### Task 6: Persist Upload Success And Clean Retained Task Output

**Files:**
- Modify: `src/agent/store.rs`
- Modify: `src/agent/server.rs`

**Interfaces:**
- Produces: `AgentStore::mark_task_succeeded(task_id) -> Result<()>`
- Produces: `AgentStore::cleanup_succeeded_tasks(retention_days) -> Result<usize>`
- Produces internally: `AgentStore::cleanup_succeeded_tasks_at(retention_days, now) -> Result<usize>`
- State file adds `finished_at` only for terminal success.
- Failed tasks are never removed by this cleanup.

- [ ] **Step 1: Write failing persistence and cleanup tests**

Add tests to `src/agent/store.rs`:

```rust
#[test]
fn mark_task_succeeded_records_finished_at() {
    let dir = tempdir().unwrap();
    let store = AgentStore::new(dir.path().join("agent_data"), None, "http://core/api".to_string()).unwrap();
    std::fs::create_dir_all(store.task_dir("task-1")).unwrap();

    store.mark_task_succeeded_at("task-1", "2026-07-01 00:00:00").unwrap();

    let state: serde_json::Value = serde_json::from_slice(
        &std::fs::read(store.task_dir("task-1").join("state.json")).unwrap()
    ).unwrap();
    assert_eq!(state["status"], "SUCCEEDED");
    assert_eq!(state["finished_at"], "2026-07-01 00:00:00");
}

#[test]
fn cleanup_removes_only_expired_succeeded_tasks() {
    let dir = tempdir().unwrap();
    let store = AgentStore::new(
        dir.path().join("agent_data"),
        None,
        "http://core/api".to_string(),
    ).unwrap();
    for task_id in ["old-success", "recent-success", "old-failed"] {
        std::fs::create_dir_all(store.task_dir(task_id).join("output")).unwrap();
        std::fs::write(store.task_dir(task_id).join("output/data.csv"), b"data").unwrap();
    }
    store.mark_task_succeeded_at("old-success", "2026-07-01 00:00:00").unwrap();
    store.mark_task_succeeded_at("recent-success", "2026-07-09 00:00:00").unwrap();
    std::fs::write(
        store.task_dir("old-failed").join("state.json"),
        serde_json::json!({
            "status": TaskStatus::Failed,
            "finished_at": "2026-07-01 00:00:00",
        }).to_string(),
    ).unwrap();

    let now = chrono::NaiveDateTime::parse_from_str(
        "2026-07-10 00:00:00",
        "%Y-%m-%d %H:%M:%S",
    ).unwrap();
    let deleted = store.cleanup_succeeded_tasks_at(7, now).unwrap();

    assert_eq!(deleted, 1);
    assert!(!store.task_dir("old-success").exists());
    assert!(store.task_dir("recent-success").exists());
    assert!(store.task_dir("old-failed").exists());
}
```

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test agent::store::tests -v`

Expected: compilation fails because success timestamp and cleanup methods do not exist.

- [ ] **Step 3: Implement timestamped success state**

Add:

```rust
pub fn mark_task_succeeded(&self, task_id: &str) -> Result<()> {
    let finished_at = crate::timeutil::now().format("%Y-%m-%d %H:%M:%S").to_string();
    self.mark_task_succeeded_at(task_id, &finished_at)
}

fn mark_task_succeeded_at(&self, task_id: &str, finished_at: &str) -> Result<()> {
    let task_dir = self.task_dir(task_id);
    std::fs::write(
        task_dir.join("state.json"),
        serde_json::json!({
            "status": TaskStatus::Succeeded,
            "finished_at": finished_at,
        }).to_string(),
    )?;
    Ok(())
}
```

Keep `update_task_state` for non-success states.

- [ ] **Step 4: Implement cleanup**

`cleanup_succeeded_tasks` must:

- scan only `data_dir/tasks/*/state.json`;
- require `status == "SUCCEEDED"`;
- parse `finished_at` with `%Y-%m-%d %H:%M:%S`;
- delete the whole task directory only when age is at least `retention_days`;
- skip malformed/unreadable state with a warning;
- never delete `FAILED` tasks;
- return the number of deleted task directories.

Use these exact signatures:

```rust
pub fn cleanup_succeeded_tasks(&self, retention_days: u64) -> Result<usize> {
    self.cleanup_succeeded_tasks_at(retention_days, crate::timeutil::now().naive_local())
}

fn cleanup_succeeded_tasks_at(
    &self,
    retention_days: u64,
    now: chrono::NaiveDateTime,
) -> Result<usize> {
    // Implement the behavior listed above.
}
```

- [ ] **Step 5: Start cleanup at Agent startup and on interval**

In `run_agent_server`, when transfer is enabled:

```rust
if let Err(error) = store.cleanup_succeeded_tasks(transfer_config.success_retention_days) {
    tracing::warn!("[agent] startup output cleanup failed: {error:#}");
}

let cleanup_store = store.clone();
let retention_days = transfer_config.success_retention_days;
let cleanup_interval = transfer_config.cleanup_interval_hours;
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(cleanup_interval * 60 * 60));
    interval.tick().await;
    loop {
        interval.tick().await;
        match cleanup_store.cleanup_succeeded_tasks(retention_days) {
            Ok(deleted) => tracing::info!("[agent] output cleanup completed: deleted_tasks={deleted}"),
            Err(error) => tracing::warn!("[agent] output cleanup failed: {error:#}"),
        }
    }
});
```

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test agent::store::tests -v
cargo test
```

Expected: all tests pass.

- [ ] **Step 7: Commit retention cleanup**

```bash
git add src/agent/store.rs src/agent/server.rs
git commit -m "feat(agent): clean retained successful task outputs"
```

---

### Task 7: Add Integration Tests For Package-Level Visibility

**Files:**
- Modify: `src/agent/transfer/mod.rs`
- Test: `src/agent/transfer/mod.rs`

**Interfaces:**
- Verifies two packages under the same first-level directory do not affect each other.
- Verifies old `_SUCCESS` removal is scoped to the exact second-level package.
- Verifies nested relative paths remain intact.

- [ ] **Step 1: Add same-level repeated directory regression test**

```rust
#[test]
fn repeated_first_level_directory_keeps_second_level_packages_isolated() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("output");
    for package_name in [
        "LTE_PM_1604007_202606171445",
        "LTE_PM_1604008_202606171445",
    ] {
        let package = output
            .join("tpd_eutr_prb_q_2026061714")
            .join(package_name);
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("data.csv"), package_name).unwrap();
    }

    let packages = discover_output_packages(&output, "/core/uploads").unwrap();

    assert_eq!(packages.len(), 2);
    assert_ne!(packages[0].remote_dir, packages[1].remote_dir);
    assert!(packages.iter().all(|package| {
        package.remote_dir.starts_with("/core/uploads/tpd_eutr_prb_q_2026061714/")
    }));
}
```

- [ ] **Step 2: Add marker scope regression test**

Use `RecordingBackend` to upload both packages and assert the removed/created marker paths are exactly:

```text
/core/uploads/tpd_eutr_prb_q_2026061714/LTE_PM_1604007_202606171445/_SUCCESS
/core/uploads/tpd_eutr_prb_q_2026061714/LTE_PM_1604008_202606171445/_SUCCESS
```

Assert no operation targets:

```text
/core/uploads/tpd_eutr_prb_q_2026061714/_SUCCESS
```

- [ ] **Step 3: Run transfer test suite**

Run: `cargo test agent::transfer -v`

Expected: all package isolation tests pass.

- [ ] **Step 4: Commit package visibility tests**

```bash
git add src/agent/transfer/mod.rs
git commit -m "test(agent): cover output package isolation"
```

---

### Task 8: Document Deployment And Perform End-To-End Verification

**Files:**
- Modify: `docs/core-agent-test-guide.md`
- Modify: `部署文档.md`

**Interfaces:**
- Documents `[transfer]` fields and protocol defaults.
- Documents Core directory consumption rule: only process second-level directories containing `_SUCCESS`.

- [ ] **Step 1: Document configuration fields**

Add a table covering:

```text
enabled                       是否启用上传，默认 false
protocol                      ftp 或 sftp
host                          Core 文件服务地址
port                          FTP 默认 21，SFTP 默认 22
username/password             明文认证配置，日志不输出密码
remote_prefix                 统一远程根目录，例如 /core/uploads
retry_count                   默认 3 次总尝试
retry_interval_seconds        默认 5 秒
connect_timeout_seconds       默认 10 秒
operation_timeout_seconds     默认 60 秒
success_retention_days        默认 7 天
cleanup_interval_hours        默认 24 小时
ftp_passive                   首版必须为 true
```

- [ ] **Step 2: Document exact path example**

Use this exact mapping:

```text
本地：
agent_data/tasks/<task_id>/output/
└── tpd_eutr_prb_q_2026061714/
    └── LTE_PM_1604007_202606171445/
        ├── tpd_eutr_prb_q.csv
        ├── tpd_eutr_prb_q.ini
        ├── load.ctl
        └── result.csv

远程：
/core/uploads/
└── tpd_eutr_prb_q_2026061714/
    └── LTE_PM_1604007_202606171445/
        ├── tpd_eutr_prb_q.csv
        ├── tpd_eutr_prb_q.ini
        ├── load.ctl
        ├── result.csv
        └── _SUCCESS
```

明确说明：一级目录可重复，二级目录唯一，`_SUCCESS` 位于二级目录。

- [ ] **Step 3: Run automated verification**

Run:

```bash
cargo fmt --check
cargo test
cargo build --release --locked
```

Expected:

- formatting check passes;
- all unit tests pass;
- Core、Agent 和 parser release binaries build successfully.

- [ ] **Step 4: Run FTP smoke test**

Prepare an FTP test service with a writable `/core/uploads` root, enable `[transfer]` with `protocol = "ftp"`, run one Agent task and verify:

- remote first-level directory is created;
- each second-level package preserves all nested files;
- `.part` files are absent after success;
- `_SUCCESS` exists only inside each second-level package;
- rerunning the same package atomically overwrites files;
- task reports `SUCCEEDED` only after marker creation.

- [ ] **Step 5: Run SFTP smoke test**

Repeat the same checks with `protocol = "sftp"`, port 22 and password authentication.

- [ ] **Step 6: Verify failure behavior**

Use an invalid password or remove remote write permission and verify:

- connection/upload is attempted exactly 3 times by default;
- logs include protocol, host, port, username and attempt number but not password;
- local task state becomes `FAILED`;
- Core receives `FAILED` with no result rows;
- local output remains present;
- no `_SUCCESS` is created for the failed package.

- [ ] **Step 7: Verify retention behavior**

Use a temporary `success_retention_days = 0` test configuration or deterministic unit test fixture and verify:

- successful task directories are removed by startup/periodic cleanup;
- failed task directories remain;
- cleanup does not inspect or remove unrelated paths outside `data_dir/tasks`.

- [ ] **Step 8: Commit documentation and verification updates**

```bash
git add docs/core-agent-test-guide.md 部署文档.md
git commit -m "docs(agent): document output transfer deployment"
```

---

## Final Verification Checklist

- [ ] Missing `[transfer]` remains backward compatible and disables output upload.
- [ ] Invalid enabled configuration prevents Agent startup.
- [ ] FTP and SFTP use the configured host, port, username and password.
- [ ] Password never appears in tracing output or formatted errors.
- [ ] `output/` first-level directories may repeat without collisions.
- [ ] Every second-level directory becomes an independent remote package.
- [ ] Remote path is exactly `remote_prefix/<level1>/<unique-level2>`.
- [ ] Nested files retain relative paths below the second-level directory.
- [ ] Direct files above the second level are ignored.
- [ ] Symlinks and non-regular files are ignored and never followed.
- [ ] Old `_SUCCESS` is removed only from the exact second-level package being replaced.
- [ ] Each file uploads to `.part` and is renamed over the final path.
- [ ] `_SUCCESS` is created last and marker creation failure fails the task.
- [ ] Empty output succeeds without opening a remote connection.
- [ ] Package upload retries exactly the configured number of attempts.
- [ ] Parse success plus upload failure reports task `FAILED`.
- [ ] Successful local output is retained for the configured number of days.
- [ ] Failed local output is never removed by automatic cleanup.
- [ ] `cargo fmt --check`, `cargo test`, and `cargo build --release --locked` pass.
