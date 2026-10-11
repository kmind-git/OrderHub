# P3 后续实施与分批验收

日期：2026-09-30。状态：本批次验收完成，P3 整体仍进行中。未提交、推送或执行外部发布。
本批次未启动 PostgreSQL/Redis 服务；保留后端的真实服务验证在启用前进行。

## 已实施

1. infrastructure 移除 live、persistence、serialization 开发依赖。
   PostgreSQL 自定义数据测试改用本地 JSON 夹具，保留原类型名、字段、元数据、标识及完整相等断言。
   增加无需服务的 envelope/往返测试；两个 Redis 节点集成测试迁到 live，保留原断言、时限、恢复场景。
   两个迁移测试仍匹配 nextest 的 `serial_tests` 组，并共享进程内锁，避免同一测试服务并发清理。
   Windows 编译完整测试体，但默认忽略这两个需要 Linux 专用 Redis 服务的测试。
2. testkit 默认只启用 `cache`。`testers` 显式包含 `cache`；研究调用方仍显式选择
   `datasets`/`testers`，其删除由 P3/P7 的消费者迁移决定。
3. 新增 Rust `orderhub-check dependency-features` 和 `test-network`。
   检查 feature 扩张、test-support 归属、lockfile 一致性、测试字面量外网访问和 live opt-in。
   读取未提交的新 manifest，并拒绝不存在的检查根路径。
   Make、pre-commit、CI 已切换；原四个 Python 检查/测试文件移到本目录 `reference/` 留作来源和对照材料。
4. 通用 CLI crate 与安装器退役。PostgreSQL bootstrap、Make/开发说明已使用 `orderhub-db`。
   旧 CLI/Cargo 发布脚本及专属测试退役，未执行这些脚本或任何远端操作。
   历史 catalog 的 Rust migration API 及 persistence 数据兼容测试仍保留。
5. 同步操作说明、CI、AGENTS、CONTRIBUTING、Make 测试库存。
   修复库存检查缺失 `ADAPTER_CRATES` 定义的问题；当前无 adapter workspace 成员，显式定义为空。
   Cargo.lock 由 Cargo 更新，已不包含 `nautilus-cli`。

## 验收证据

| 检查 | 结果与边界 |
| --- | --- |
| stable `cargo fmt --all -- --check` | 通过。 |
| `cargo check --offline -p orderhub-tools --all-targets` | 通过，并由 Cargo 更新 lockfile。 |
| `cargo clippy --locked --offline -p orderhub-tools --all-targets -- -D warnings` | 通过。 |
| `cargo test --locked --offline -p orderhub-tools --bins --profile dev` | 8/8 通过：数据库参数/schema 4 项、Rust 检查器 4 项。使用无 debug symbols 的 dev profile。 |
| infrastructure postgres+redis 的 all-targets check/Clippy | 通过；不代表 Linux 服务测试已经运行。 |
| infrastructure 自定义数据 integration 测试 | 1/1 通过，验证完整 JSON envelope 及往返相等。 |
| live `--no-default-features --features node --test integration` Clippy | 通过，包括两个迁移后的 Redis 测试体。未运行服务测试。 |
| testkit `--no-default-features --features cache --lib` check/Clippy | 通过。 |
| 同输入迁移对照 | 9 组依赖配置、34 个网络/发现样本，以及仓库两项扫描结果一致。见 [对照结果](checker-parity.json) 和 [最终仓库复核](checker-parity-final.json)。 |
| 两个 Rust 检查命令 | 通过；不存在根路径被拒绝，退出码 1。 |
| 依赖边界 | [infrastructure 测试依赖](infrastructure-test-dependencies.txt) 不包含 live/persistence/serialization/datafusion/parquet；[cache 正常依赖](testkit-cache-dependencies.txt) 不包含 trading/serialization/persistence/parquet/reqwest/aws-lc/zstd。不是最终无 C 构建结论。 |
| PowerShell 打包 | `target/orderhub-db-package-p3-cli-retired` 通过版本/schema 检查；四个 SQL 文件 SHA-256 与源文件一致。 |
| Bash 语法 | `bash -n` 校验 cooldown 测试与数据库打包脚本通过。Git Bash 的沙箱信号管道限制通过已批准的独立语法检查处理。 |
| workspace 测试库存 | 使用 `python/.venv` 执行现有脚本的原始嵌入式检查，26 个成员完整。该 Python 检查仍需后续迁移。 |
| 文档链接及 diff | 变更文档的文件链接与 `git diff --check` 通过；网站绝对路径不是本地文件目标。 |

当前环境未发现可调用的 GNU Make、ShellCheck 或 taplo，未将完整 Make 目标、ShellCheck 或 taplo 记为通过。
本批次没有重跑未修改的整个交易业务矩阵，也没有重新宣称 P1/P8 的完整链接、运行时或最终无 C 验收。

## 修正与恢复材料

初版新增 JSON 测试误认为 payload 仍含 `type` 字段；实际宏将类型放在外层 envelope，已核实并修正期望。
迁移对照发现 Rust URL 解析器错误拒绝动态端口的 loopback 地址；改为与原检查一致的 hostname/IP 规则后对照通过。

最初的批量清理被自动审批拒绝，理由是范围较大且仅有哈希不能恢复源码。
随后先保存并逐文件校验完整源码，再按明确的 CLI 模块和已退役入口分批处理，自动审批允许这些操作。
完整恢复材料为 `retired-cli-source.zip`（19 个文件、26,362 字节），路径保留为仓库相对路径；
[退役源码哈希](retired-source-hashes.txt) 可用于核对。恢复时选择需要的文件，避免覆盖后续修改。
Python 原始检查来源位于 `reference/`；`compare-checks.py` 和 `checker-parity-inputs.zip`
只作为一次性审计材料，不属于仓库的 Python 执行门槛。

## P3 尚未核销

- 其余必要 Python 检查/运维编排及 shell 中嵌入的 Python；包括安全审计、测试库存、提交信息等规则。
- 插件 guest SDK、LiveNode 宿主兼容桩及其配置/文档消费者。
- 通用 Python 源码、分散 Rust 绑定、生成器和通用示例；先核对/迁移保留测试与工具消费者。
- 相关 feature、补丁、Python 环境配置与锁文件的最终清理。
- 研究 fixtures、重型格式和生命周期消费者的后续迁移，与 P7 按依赖推进。

因此主方案的 P3 保持未勾选。数据库实例暂未启用不阻塞上述本地裁剪工作。
