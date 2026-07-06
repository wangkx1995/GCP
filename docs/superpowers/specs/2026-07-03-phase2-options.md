# Phase 2 方案对比

## 背景

Phase 1 已完成：Core 支持 zip 上传 → 校验 → 解包 → 版本管理 → 激活/回滚。

Phase 2 的目标是让 Agent 能获取到 Core 上的配置。当前 Agent 已有 `--config-dir` 机制，但仅限本地共享文件系统，不适用于多机器部署。

---

## 方案 A：SFTP 分发（原 Spec）

**工作流：**
- Core 起 SFTP server，`config_storage/active` 作为 SFTP chroot
- Agent 通过 `config_sftp.toml` 配置 SFTP 连接信息
- Agent 在收到 task 时或定期通过 SFTP 拉取配置

**优点：**
- 符合原 Spec 设计
- SFTP 天然支持增量/断点

**缺点：**
- 需要引入 SFTP server 库或依赖系统 SFTP（增加运维）
- 端口/防火墙/密钥管理额外开销
- `ssh2` 已有 vendored-openssl 依赖，但 SFTP server 端不同

**工作量：** ~2-3 天

---

## 方案 B：HTTP 直连分发（推荐）

**工作流：**
- Agent 通过 HTTP `GET /api/config-snapshots/{id}/download` 拉取配置 zip
- Agent 收到 task 时，如果本地没有对应 snapshot，自动调用 Core API 下载
- Core 返回 zip，Agent 解包到本地 `data_dir/configs/{snapshot_id}/`

**优点：**
- **零新依赖** — 利用现有 reqwest/Core API
- Agent 已有 HTTP client 和 Core API base URL 配置
- 与 task dispatch 流程自然集成：task 指定 `config_snapshot_id` → Agent 检查本地 → 缺失则拉取
- 架构简单，debug 方便 (curl 即可测试)

**缺点：**
- HTTP 传输大文件效率低于 SFTP（但配置 zip 通常 <1MB，可忽略）
- 缺少增量同步（但配置 zip 是整体替换，不需要增量）

**工作量：** ~1 天

---

## 方案 C：配置热更新（可在 A 或 B 之上叠加）

**工作流：**
- Core 激活新配置时，遍历所有 ONLINE Agent
- 向 Agent 发送 `POST /api/configs/update` 通知（含 snapshot_id + content_hash）
- Agent 收到后异步拉取新配置，校验 hash，准备就绪后回复 Core

**优点：**
- 配置变更即时生效，无需等待下次 task 调度
- 适合需要紧急下发配置的场景

**缺点：**
- 依赖底层传输（A 或 B），不是替代方案
- 需要 Agent 侧新端点 + Core 侧通知逻辑
- Agent 更新期间正在运行的任务需要策略（允许跑完/强制中断）

**工作量：** ~1 天（在 B 的 HTTP 分发基础上）

---

## 推荐路径

1. **先做方案 B（HTTP 直连）** — 1 天，最小成本解决多 Agent 配置分发问题
2. **再做方案 C（热更新通知）** — 1 天，在 B 基础上叠加，Core 激活时推送
3. **方案 A（SFTP）** 暂缓，除非有明确的外部 SFTP 对接需求

B 和 C 加起来约 2 天，即可获得完整的配置分发能力。
