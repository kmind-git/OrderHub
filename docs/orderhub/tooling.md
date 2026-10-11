# OrderHub 专用工具迁移

状态：P3 已按限定范围验收。本文件记录从通用研究工具迁出后的真实入口与保留消费者，
不表示 PostgreSQL 服务操作已经验收。主方案见
[项目精简方案](../../orderhub-project-simplification.md)。

## PostgreSQL schema 入口

专用 Rust 入口为 `orderhub-db`，位于 `crates/orderhub-tools`。它复用当前
`nautilus-infrastructure::sql::pg::init_postgres`，所以仍执行同一组已有的
`schema/sql/{types,tables,functions,partitions}.sql`，没有复制迁移实现。

```bash
cargo run --locked --offline -p orderhub-tools --bin orderhub-db -- --version
cargo run --locked --offline -p orderhub-tools --bin orderhub-db -- \
  schema-check --schema schema/sql
```

`schema-check` 只读四个 SQL 文件，拒绝缺失、空文件或非 UTF-8，输出文件字节数与
SHA-256，可在没有 PostgreSQL 服务的机器上运行。`init --schema PATH` 先进行
相同检查，再调用保留的初始化/迁移函数。必须通过参数或环境变量指定
`POSTGRES_USERNAME`、`POSTGRES_DATABASE`，密码只接受 `POSTGRES_PASSWORD`；
不能无参数落到旧 CLI 的默认账号/库。`--host` 和 `--port` 可省略。
`init` 只用于明确指定的环境，本期没有连接服务验证。

`scripts/orderhub/package-db-tool.bash OUTPUT_DIRECTORY` 或 Windows 上的
`scripts/orderhub/package-db-tool.ps1 -OutputDirectory PATH` 构建二进制并复制四个
SQL 文件到新目录，随后运行 `--version` 和 `schema-check`；已存在的输出目录
会被拒绝。Bash 可设置 `ORDERHUB_DB_PROFILE=dev`，PowerShell 可传入
`-Profile dev` 或 `-Profile nextest` 做本地打包检查；默认使用 release。
`nextest` 保留断言和溢出检查并省略大体积调试符号，可复用本地验收产物。
打包内容包括 `bin/orderhub-db[.exe]` 和 `schema/sql/`；运行时显式传入后者。

通用 `nautilus-cli` 已退役：PostgreSQL bootstrap 改用专用 `init`，
Make/开发说明改为专用构建和打包；旧安装器、CLI/Cargo 发布脚本及其测试已退役。
P7 将历史数据兼容读取迁到独立 `orderhub-migrate`；用法和边界见
[历史迁移入口](framework-retirement.md#离线历史迁移)。P3 的完整源码备份见
[P3 后续记录](../../audit/orderhub-simplification-2026-09-30/run-11/P3-progress.md)。
真实初始化、重复迁移、权限和 schema 行为
留到启用数据库前以专用测试实例验证，不能用离线文件检查代替。

## Rust 仓库检查

当前 `.github/workflows/ci.yml` 只运行 Rust 构建、lint、测试和依赖审计，
Makefile、pre-commit 已改为 Rust 工具及原生检查入口。迁移状态如下。

| 当前入口                | 验证与处置                                                                                                        |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------- |
| 依赖 feature 策略检查   | 已迁至 Rust `orderhub-check dependency-features`；9 组配置与当前仓库对照一致。                                    |
| 测试网络检查            | 已迁至 Rust `orderhub-check test-network`；34 个样本与当前仓库对照一致。                                          |
| 安全审计                | `orderhub-check security validate/check-tools/run`；验证策略、路径类型、工具版本和审计命令。`report` 不豁免失败。 |
| Markdown 表格与文档链接 | `markdown-tables PATH...`、`doc-links OUTPUT`；保留文件字节和链接清单语义。                                       |
| 提交消息                | `commit-message PATH [--raw]`；保留错误、长度提示、Git 注释和 scissors 清理。                                     |
| 工作区测试与退役边界    | `workspace-tests`、`retired-surfaces`；核对清单、Python/插件及 FFI 所有权。                                       |
| Python 绑定生成         | 绑定、源码、生成器、配置和锁文件已按消费者映射退役。                                                              |
| 严格 Clippy 报告        | 13 条规则保持在 Make，Cargo 直接输出 JSON，编译错误仍失败。                                                       |
| shell 中的嵌入式 Python | 已迁为 jq/Cargo metadata；保留路径与依赖更新测试。                                                                |

`orderhub-check` 的轻量构建不启用 database feature，Make、pre-commit 和 CI
均使用这一 Rust 入口。原 Python 检查与测试移入审计参考材料，不再作为执行入口。
它还检查未提交的新 manifest，避免新 crate 尚未加入 Git 时漏检。

infrastructure 已移除 live、persistence、serialization 开发依赖：PostgreSQL
自定义数据改用同 wire 字段的本地 JSON 夹具，保留元数据/标识/完整相等断言；
Redis 双总线流入与 Cache 重启恢复测试位于 infrastructure/redis_restoration，仍属 serial_tests 组，
并共享进程内锁。测试在 Windows 编译，实际服务运行留到启用前验证。
testkit 仅保留 `cache` 和事件收集；研究 datasets/testers 已在 P7 退役。

新增工具的 68 个同输入对照通过；源码恢复、精确删除清单与必要断言映射见
[消费者退役记录](../../audit/orderhub-simplification-2026-10-01/run-13/consumer-retirement.md)。
P3 当时保留的回测测试和 Arrow 生成器已经在 P7 退役；旧夹具继续由独立迁移工具验证。
本机忽略的 `python/.venv` 保留，支持的构建、检查和 schema 入口均不使用它。
`markdown-tables` 默认保留旧工具的兼容宽度；文档门禁使用 `--display-width`，
按中文和 emoji 的显示宽度对齐当前 MD060。该新增模式另有回归检查。
本阶段证据见 [P3 验收记录](../../audit/orderhub-simplification-2026-10-01/run-13/P3-acceptance.md)。
数据库实机、研究大数据及最终无 C 门槛仍按其负责阶段验证。
