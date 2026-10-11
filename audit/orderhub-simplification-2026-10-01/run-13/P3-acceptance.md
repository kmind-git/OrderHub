# P3 最终验收

日期：2026-10-01。状态：P3 按本阶段限定范围验收通过。

本阶段按主方案第 3 节的消费者迁移和第 4 节 P3 范围核销。
PostgreSQL/Redis 真实服务行为留到启用前验证，研究数据兼容和全部保留路径的
无 C 构建、链接、运行证据由 P7/P8 核销。

## 实施范围

- 专用 `orderhub-db` 保留 schema、初始化和迁移入口；参数校验、版本、离线 schema 检查及打包验证无需数据库。
- 必要工具迁为 Rust `orderhub-check`：依赖 feature、测试网络、提交消息、表格、文档链接、安全策略、工作区清单和退役边界。
- 安全工具的 `report` 只决定报告方式，非零执行结果仍失败；13 条可选严格 Clippy 规则保持在 Make。
- 绑定、插件、示例和 Python API 按消费者迁移退役；Rust 业务断言及数据库/Redis 测试材料保留。
- Shell 的元数据、路径及依赖更新检查使用 Bash/jq/Cargo，不依赖 Python。
- Make、hooks、CI、维护说明和开发文档同步为 stable Rust；193 份保留研究/API 文档明确标记为历史材料。

分批证据包括 [专用数据库入口](../../orderhub-simplification-2026-09-30/run-10/P3-progress.md)、
[基础库与工具迁移](../../orderhub-simplification-2026-09-30/run-11/P3-progress.md)、
[绑定与示例裁剪](../../orderhub-simplification-2026-09-30/run-12/P3-example-retirement.md)。
本批职责与断言处置见 [消费者退役映射](consumer-retirement.md)。

## 精确删除与恢复

用户已明确确认本批 431 个文件以及最后两个配置文件，共 433 个源码/配置文件。
删除前逐文件检查路径、哈希和备份内容；先完成消费者迁移，再删除
`python/pyproject.toml`、`python/uv.lock`。

- [删除清单](retirement-manifest.json) 包含路径、原因、大小和 SHA-256。
- [本批恢复归档](retirement-source.zip) 保留 431 个文件的精确字节。
- [完整源码快照](source-before.zip) 与 [原始哈希](source-before-hashes.json) 对照通过，包含 2297 个文件。
- [最后两个配置的删除记录](retired-python-config.json) 记录原始哈希。
- `LICENSES/` 保留 Python 包及 PyO3 stub generator 的许可证，原始源码通知也在归档中。
- `python/.venv` 保留且被 Git 忽略；它不参与受支持的 Rust 构建或检查。

恢复时只取需要的文件，先比较本地较新修改，不用整个快照覆盖当前工作区。

## 最终验证

实际命令、预期与实际退出码见 [验证清单](final-validation.json)，逐项日志在本目录。
同输入对照使用恢复归档中的原工具作为参考，参考程序只在审计目录执行。
生产、开发和 CI 入口均使用 Rust 工具。

工具链为 stable Rust 1.98.1，主机 `x86_64-pc-windows-msvc`，见 [工具链记录](toolchain.txt)。
本期验证只针对该主机；没有将其他操作系统的构建记为通过。

| 验证范围         | 实测结果与证据                                                                                                                                                                                                |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 工作区 Rust 格式 | `cargo fmt --all -- --check`，退出码 0，见 [日志](format.log)                                                                                                                                                 |
| 工作区 Clippy    | `--workspace --all-targets --features nautilus-persistence/fixture-tools -- -D warnings`，退出码 0，见 [日志](workspace-clippy-final.log)                                                                     |
| 四个 OrderHub 包 | `cargo check --all-targets` 退出码 0；`cargo test --profile nextest --lib --bins` 17 个测试通过，0 失败/忽略/过滤，见 [日志](orderhub-tests.log)                                                              |
| 当前仓库策略     | dependency-features、test-network、workspace-tests、retired-surfaces、security validate 全部退出码 0，命令见 [清单](final-validation.json)                                                                    |
| 数据库工具       | schema/version 均为 0；未知选项、非法端口和命令行密码均按预期拒绝为 2；PowerShell nextest 打包的版本/schema 均通过，见 [打包日志](database-package.log)                                                       |
| 同输入对照       | 68/68 通过，比较错误、提示、文件字节、命令参数和退出码，见 [逐例结果](tool-parity.json)、[输入归档](parity-inputs.zip)                                                                                        |
| 审计失败传播     | 固定返回 7 的隔离模拟工具，在 report=false/true 两种配置下均使策略检查返回 1，2/2 通过，见 [结果](security-failure-validation.json)                                                                           |
| Shell 入口       | 60 份语法通过；Windows 元数据正确解析 5 个包；路径、更新事务和版本测试复用输入未变的通过记录；全量离线冷却和新版本拒绝测试通过，见 [日志](shell-validation-final.log)及[退出码](shell-background-result.json) |
| 依赖日期生成     | 686 个版本满足三天策略；24 条日期经 crates.io 取得，清除 193 条失效记录，见 [生成日志](cooldown-database-refresh-final.log)                                                                                   |
| Markdown         | 当前 14 份文档及 193 份历史文档均为 0 问题，见 [当前文档日志](markdown-lint-final.log)、[历史文档日志](historical-markdown-lint.log)                                                                          |
| 配置/文件链接    | 两份 YAML 可解析；14 份文档的本地文件目标均存在，见 [范围与结果](doc-config-validation.json)；不表示外部链接或完整 lychee 检查通过                                                                            |

冷却策略的首次原生 jq 比较遇到 Windows CRLF/LF 差异。
原始 686 行全部带 CRLF，规范化后两边键完全一致，见 [诊断](cooldown-key-normalization.json)。
测试只规范化传输行尾、固定排序，保留逐键比较和离线/新发布版本拒绝断言；最终重验退出码 0。

## 最终文件状态与产物

433 个目标文件均不存在；四个回测文件和新的 Arrow 生成器与本批开始时字节/哈希一致，
许可证和历史 `.venv` 保留，见 [文件状态](retirement-state.json)。
最终 1868 个保留源文件 SHA-256 见 [完整清单](current-source-hashes.json)，
验证二进制及数据库工具包的哈希见 [清单](verification-binary-hashes.json)。
清单覆盖实际文件状态，包含未提交文件；Git HEAD 本身不代表这次验证的文件状态。

已删除 `target/debug`、`target/nextest`，目录逻辑文件大小合计 10,925,978,806 字节。
D 盘可用空间实测增加 10,252,394,496 字节，约 9.55 GiB；
恢复归档、日志和独立数据库工具包保留，见 [实测记录](artifact-cleanup.json)。

## 验证边界

本批没有连接 PostgreSQL/Redis，没有执行删库操作，没有运行实时行情或实盘交易。
没有运行完整安全公告审计，也没有将策略解析成功记为安全审计通过。
本机完整 prek 及部分可选原生格式/扫描工具不可用，见 [可用性清单](native-tool-availability.json)；
实际执行的替代检查逐项记录，
不声明完整 `make pre-commit` 已通过。保留历史文档没有作为完整研究环境验收。

run-12 证明四个回测文件的 63 个测试函数名及 147 处完整断言被保留，
54 个夹具测试和 6 个内存场景通过；缺失大数据的 3 个 ITCH 场景只编译，执行由 P7 核销。
Arrow 工具的五份生成输出在该批与既有夹具字节/哈希一致；本批未改变生成算法。

P4-P6 的行情、交易、恢复契约仍待实现。P3 通过不表示这些阶段已通过，
也不代表最终无 C 或实盘上线验收。没有提交、推送或发布。
