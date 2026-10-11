# P7 验收记录

收尾日期：2026-10-10。状态：P7 本期框架裁剪与历史数据兼容验收通过。
P7 实施和核心检查记录在本目录；源码变更后重跑受影响检查。
本阶段核销重型框架与数据兼容消费者，最终应用 TLS、证书及无 C 构建/链接/运行矩阵属于 P8。
路线见 [精简方案](../../../orderhub-project-simplification.md)，命令和限制见
[框架退役与历史迁移](../../../docs/orderhub/framework-retirement.md)。

## 实现及消费者核销

| 原模块或接口                    | 处置与验证                                                                                                                                               |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| live、system、event_store       | 通用节点与生命周期框架退役；OrderHub node/gateway 承接 P4–P6 生命周期、检查点、备份与恢复；重验真实进程故障与 FIX 闭环。                                 |
| analysis                        | 从 portfolio 解除研究统计依赖；保留账户、持仓和组合估值；已实现盈亏采用原生 Money，保持币种与精确数值。                                                  |
| backtest、indicators、trading   | 研究、回测和内部策略框架退役；保留 OMS 核心、外部调用协议及核心消息回调接口。                                                                            |
| persistence、serialization      | 通用 catalog/query 框架退役；39 个必要手写 Arrow 编解码源文件迁到可选 orderhub-tools/legacy-catalog；JSON/MsgPack/prost 路径继续保留。                   |
| crates 根容器                   | 删除无消费者的聚合 Cargo/lib/README；内部 nautilus-* 名称保持不变。                                                                                      |
| data streaming、Cap'n Proto/SBE | 移除通用 streaming、schema 生成器和二进制 codec 消费者；不支持的旧 codec 返回明确错误，OMS 行情路径保留。                                                |
| cryptography                    | 保留纯 Rust HMAC/Ed25519 与 base64；移除无消费者 RSA/provider/TLS 汇聚接口；应用 TLS 与证书要求由 P8 完成最终接入及验证。                                |
| testkit                         | 保留 Cache 内存替身与事件收集，移除研究 datasets、测试 actor/strategy 和下载工具。                                                                       |
| Redis 恢复测试                  | 迁到 infrastructure 的 redis_restoration；直接验证 MessageBus 与 Cache 路径，保留账户/订单/持仓/索引/禁用加载/清空恢复断言；编译通过，真实服务执行延期。 |
| feature、脚本、CI、文档         | Makefile、features、changed hooks、CI 和策略检查指向当前成员；移除 Cap'n Proto 安装与生成依赖；旧开发文档显式标为历史参考。                              |

workspace 从 P7 开始时的 **25 个成员缩减为 15 个**，9 个重型 crate 目录及根容器均已退役。
当前成员见 [Cargo metadata](final-metadata.json)。清单中的 38 个无消费者 workspace 依赖声明已移除，
清单见 [unused-dependencies.json](unused-dependencies.json)。锁文件由 Cargo 解析更新。

共删除 **488 个有逐文件备份的原文件**：框架 457、旧 codec 9、旧密码接口 2、重型 testkit 20。
完整清单见 [all-retired-files.json](all-retired-files.json)，各批次归档核验和删除结果见
[框架](retired-framework.json)、[codec](retired-codecs.json)、[密码](retired-crypto.json)、[testkit](retired-testkit.json)。
删除前均核对当前内容与归档 SHA-256，删除路径限制在本仓库。

## 历史数据兼容

- 新入口为 `orderhub-migrate parquet` 和 `verify`，仅显式启用 `legacy-catalog` 时构建；默认节点不启用 Arrow/Parquet。
- 支持内建行情、账户、订单/持仓事件、报告、快照与合约；拒绝未知 schema/任意 CustomData。
- 输出版本 1 JSONL，包含源 BLAKE3、连续行序号、原生精确数值、事件数、数量/金额总计和记录摘要。
- 原始 64/128 位的 quotes、trades、bars、deltas、depths 共 10 份 Parquet 文件实际读取；逐类型比较行数、精确总计和记录摘要相等，并回验输出。
- 成交夹具保留 1.122 数量、24.40 USDT 费用、24684 名义金额和 2 个成交事件；账户夹具保留 10 BTC。
- 旧十档平铺深度显式转换，空档只接受全零尾部；拒绝中间空档和错误填充值。
- 源文件不改写；临时文件 flush/sync、验读及源摘要复核后，以同目录硬链接发布，已有目标拒绝覆盖。
- 单源 128 MiB、输出 512 MiB、100 万行、单行 1 MiB。最终修订在写入每一行前检查行与总大小，拒绝超限行写入；边界测试包括换行占用、精确上限和超限后输出不变。
- 工具不自动导入交易账本，不将状态恢复为 READY；内部摘要不是签名，需要证明源对应关系时保留原文件并核对源摘要。

Parquet 59.3.0 的本地补丁用纯 Rust ruzstd 0.9.0 替代原 Zstandard C 后端，限读取。
完整帧、连接帧、内容 checksum、页大小、64 MiB 窗口/页上限及损坏输入已测试；Zstandard 编码显式拒绝。
上游版权、Apache License、NOTICE、原始 manifest 与源校验和保留，见
[补丁说明](../../../patches/parquet-59.3.0/ORDERHUB-PATCH.md)。不声称保留的上游 Zstandard writer 测试通过。

命令行实际验证见 [8 个场景](migrate-cli-validation.json)及[源/输出/二进制摘要](migrate-cli-proof.json)：
帮助、缺失参数、转换 9,500 条 QuoteTick、回验同一摘要、拒绝覆盖、未知类型、缺失源和损坏输出。
此前检查结果也保留在 `*-before-output-limit.json`，最终结果对应写入前限额修订。

## 检查与证据

使用 stable Rust 1.98.1、Windows MSVC 和 nextest profile；未重建 target/debug。
24 项核心门禁均返回 0，6,867 次测试执行通过、0 失败；6 个 ignored 的边界见下文。
命令、返回值和耗时见 [validation.json](validation.json)，
执行脚本见 [verify-p7.ps1](verify-p7.ps1)。测试计数汇总见 [test-summary.json](test-summary.json)。

| 检查                                           | 测试执行通过数或结果 | 日志                                                                              |
| ---------------------------------------------- | -------------------- | --------------------------------------------------------------------------------- |
| fmt、workspace all-targets check/Clippy        | 0 退出               | [格式](format.log)、[check](workspace-check.log)、[Clippy](workspace-clippy.log)  |
| 历史迁移 Clippy                                | 0 退出               | [legacy-clippy.log](legacy-clippy.log)                                            |
| 历史迁移标准/高精度                            | 312 / 313            | [标准](legacy-tests.log)、[高精度](legacy-high-precision.log)                     |
| common                                         | 1,791                | [common-tests.log](common-tests.log)                                              |
| model/macros/crypto/data/portfolio/testkit lib | 3,831                | [core-tests.log](core-tests.log)                                                  |
| portfolio 集成 / data engine                   | 157 / 307            | [组合](portfolio-tests.log)、[行情](data-engine-tests.log)                        |
| 四个 OrderHub crate lib/bin                    | 26                   | [application-unit-tests.log](application-unit-tests.log)                          |
| P6 恢复 / 运维 / 真实 node/FIX                 | 16 / 1 / 2           | [恢复](p6-recovery.log)、[运维](p6-operator.log)、[FIX](p6-local-fix.log)         |
| P5 业务 / API                                  | 18 / 8               | [业务](p5-business.log)、[API](p5-api.log)                                        |
| P4 输入 / 原生 / 权限回归                      | 74 / 5 / 6           | [输入](p4-inputs.log)、[原生](native-regression.log)、[权限](auth-regression.log) |
| Redis 恢复覆盖                                 | 编译，不连接服务     | [redis-restoration-compile.log](redis-restoration-compile.log)                    |
| dependency-features / test-network             | 通过                 | [依赖](dependency-features.log)、[网络](test-network.log)                         |
| workspace-tests / retired-surfaces             | 通过                 | [测试清单](workspace-tests.log)、[退役面](retired-surfaces.log)                   |

common 的 43 个全局 logger/macros 用例使用同一个已构建测试二进制，分别在新进程中运行；
其余 1,748 个成功用例在普通单线程 harness 中运行。此前合并进程的 logger 状态冲突已留痕；
未删除断言、修改生产行为或新增 ignored 来通过测试。枚举及每个独立结果见
[verify-common.ps1](verify-common.ps1)和[common-isolated-tests.json](common-isolated-tests.json)。

6 个 ignored 不计为通过：common 原有 4 个行为用例（两项信号重入顺序、两项 Cache 成交/拒绝状态缺陷复现）；
model 1 个主动改写枚举文档的生成器；P6 1 个由父测试真实调用的杀进程辅助入口。
4 个原有 ignored 行为用例仍需按 P8 的实际启用路径核销。data engine 的 59 个其他用例及原有日志吞吐 benchmark
不属于此次变更的选择范围。标准/高精度及多配置重复执行数不代表互不重复的测试用例数。

补充检查包括迁移 CLI、无需服务的数据库 schema/参数检查、stable Rustdoc、Markdown/table/link、Bash 语法、Cargo cooldown 消费者回归
及由原生成器更新的 573 项依赖日期库。结果见 [补充命令](supplementary-validation.json)、
[Rustdoc](legacy-doc.log)、[Markdown](markdown-final.log)、[cooldown](cooldown-script-final.log)。
cooldown 更新记录见 [cooldown-db-update.log](cooldown-db-update.log)，不手工编辑生成数据库。
最终记录确认 995 项源码/配置哈希无漂移、21 份文档检查通过，全部本地文件链接可达。
文档原生检查脚本见 [verify-documents.ps1](verify-documents.ps1)，
哈希和文件状态复核见 [verify-final-state.cjs](verify-final-state.cjs)。
收尾脚本首次将预期的非法输入退出码误记为总失败；8 个场景已成功，修正后复核原证明的
源、输出与二进制哈希并仅续跑剩余检查，见 [首次结果及处置](supplementary-wrapper-first-attempt.json)。

当前 Windows workspace 全 features 并集未激活核查清单中的 C 后端，见 [feature-union.txt](feature-union.txt)。
这项依赖图检查不替代 P8 的实际链接、产物、TLS/证书和运行证据。
源码、文档、模拟器及归档的最终哈希与文件状态见 [final-state-validation.json](final-state-validation.json)、
[tested-source-hashes.json](tested-source-hashes.json)和[document-hashes.json](document-hashes.json)。

## 可恢复材料与后续边界

- 删除前 1,551 个文件的源码与原通知保存在 [pre-p7-source.zip](pre-p7-source.zip)，
  对应 [pre-p7-source-hashes.json](pre-p7-source-hashes.json)。原工程工具与生成日期库另有 `pre-p7-*` 备份。
- 历史开发文档引用的另外 7 份旧源码从未改动的 Git HEAD 归档，见
  [historical-reference-source.zip](historical-reference-source.zip)和[路径清单](historical-reference-paths.json)。
  13 个本地及该 HEAD 均不可恢复的更早路径仅保留文字标签，
  见 [unavailable-historical-reference-paths.json](unavailable-historical-reference-paths.json)，不声称这些源码已备份。
- 外部 FIX 模拟器只读使用，SHA-256 为 `CAA0DBD3FAB069EFF400D32FF107CE45B09C52B68ED3275A847A379E948AF290`；
  测试只清理自建进程，没有改动该项目。
- Redis/PostgreSQL 的认证、事务、重连和实际工具操作继续作为未来启用门禁；没有连接真实服务。
- P8 仍未验收：最终 B/R/P/PT/G/U 配置构建、链接、TLS/provider 接入、证书校验、运行矩阵及本期业务结果汇总。
- 保留其他阶段的本地编辑、许可证和来源；Git HEAD 保持 `479d8674d80af212044731ef43c9a24db30aa40f`，未提交或推送。
