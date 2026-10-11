# P3 工具迁移首批记录

日期：2026-09-30。状态：**P3 进行中，尚未验收通过**。基准提交
`479d8674d80af212044731ef43c9a24db30aa40f` 加本地现有改动。
本次只迁移 PostgreSQL schema 管理入口及相关调用，不连接 PostgreSQL/Redis，
不删除通用 CLI、插件、Python 或研究数据，也不提交/推送。

## 本批完成的工作

- 新增 `orderhub-tools` 的 `orderhub-db`，复用现有 `init_postgres` 和
  `schema/sql`。`schema-check` 离线核对四个实际文件的存在、非空、UTF-8、
  字节数与 SHA-256；`init` 必须显式选择数据库/用户并从环境取密码，
  无 `drop` 子命令。
- 将 PostgreSQL bootstrap 测试脚本的初始化调用迁入专用入口；仅清理本次
  创建的测试容器，不再把旧通用 CLI 的删库命令列入自动验收。
- 新增 Bash/PowerShell 打包脚本，复制二进制及四个 SQL 文件并离线自检。
  更新 Makefile 的工具目标、bootstrap 变更输入、CI 选择、开发说明和
  `nautilus-infrastructure` 的 schema 升级提示。
- 消费者及其未完成迁移记录在
  [工具迁移说明](../../../docs/orderhub/tooling.md)。通用 CLI 仍被安装脚本、
  Make 目标和历史脚本引用，当前不能删除。

## 本机验证

| 检查 | 结果及边界 |
| --- | --- |
| stable 1.98.1，`cargo check --locked --offline -p orderhub-tools --all-targets` | 通过；锁文件已包含新 crate 且命令未改锁。 |
| `cargo test --locked --offline -p orderhub-tools --bin orderhub-db` | 4/4，通过；仅测试参数和本地文件，不测试数据库行为。 |
| `cargo clippy --locked --offline -p orderhub-tools --all-targets -- -D warnings` | 通过。 |
| `cargo fmt --all -- --check`、`git diff --check` | 通过。 |
| `bash -n` 三份本轮修改的 Bash 脚本 | 通过；沙箱内 Git Bash 创建信号管道失败，改用只读提升权限运行。 |
| Windows PowerShell `package-db-tool.ps1 -Profile dev` | 对最终工具代码重跑打包，`--version`、`schema-check` 成功；四份包内文件哈希与系统 SHA-256 一致。 |
| Git Bash `ORDERHUB_DB_PROFILE=dev package-db-tool.bash` | 打包与离线自检成功；四份包内 SQL 与源码哈希一致。为节省空间，检查后清理了重复的 Bash 测试包，保留 PowerShell 打包结果。 |

本机没有 GNU Make，Make 目标的模拟回归脚本未运行。没有数据库实例，
`init` 的认证、角色/权限、重复迁移和真实 schema 效果均未验证；这些按
主方案留到启用 PostgreSQL 前。本次也没有把 `cargo check` 当作链接或
最终无 C 证据。另核对了 5 份相关文档中的 25 个本地链接，包内四份 SQL
文件与源码 SHA-256 一致；本批关键输入哈希见 `source-hashes.txt`。

## P3 继续项

1. 逐个迁移必要的 Python 检查/生成/运维逻辑，用同一输入比较结果；
   绑定和研究专属项在消费者核销后退役。
2. 解除 infrastructure 对 live/persistence 的测试依赖，保留有价值的
   Redis/PostgreSQL、受理、额度和恢复测试；裁剪 testkit 研究 fixtures。
3. 迁移通用 CLI、插件、示例、Python/绑定及各项脚本消费者后分批删除，
   核对 feature、补丁、锁文件及历史数据。`make init-db` 仍用旧容器内
   psql 路径，需在数据库启用验证时再决定替换。
4. 逐模块完成构建、lint、行为、打包及回退核对，然后才能勾选 P3。
