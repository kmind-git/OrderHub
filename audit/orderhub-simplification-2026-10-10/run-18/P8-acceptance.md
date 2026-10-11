# P8 最终版本验收记录

验收日期：2026-10-11。状态：本期 Windows x64 的 P8 与 C0–C6 已通过。
本记录对应当前 Windows x64 本地交付，路线与延期边界见
[精简方案](../../../orderhub-project-simplification.md)及
[无 C 子方案](../../../orderhub-tls-stack-removal.md)。
不涉及外部发布、真实交易或新的 Linux/macOS 部署。

## 本期实现

- 节点、Rust 客户端和保留数据库库使用统一 rustls/Graviola provider；真实 CPU 门禁、
  provider 冲突拒绝、显式根证书、主机名校验和禁止降级由生产接口承担。
- B 只接受显式回环明文；G 的节点/客户端必需 TLS。节点在打开日志前验证 TLS 身份。
- PostgreSQL 普通连接显式 Disable，TLS 仅接受 VerifyFull 和显式根；SQLx 本地补丁已接入生产库。
  Redis 保留普通/TLS、认证 URI、连接管理、Sentinel/Streams 接口；真实服务功能未验证。
- 正常节点入口拒绝未验收的 PostgreSQL/Redis 启用开关；构建 feature 不会打开投影或连接服务。
- CodSpeed 的构建脚本在实际哨兵下尝试编译 C；已保留失败证据并退役这条依赖。
  五个 Iai benchmark 文件的 54 项原计算/断言迁到原生 Criterion 0.7.0。
  最小 Criterion 补丁禁用外部 gnuplot；保留上游来源、许可证和本地差异。
- 保留 HMAC/Ed25519、BLAKE3 pure、Parquet/ruzstd 历史读取和必要 Rust/prost 生成入口。
  两个原 Cache 成交/拒绝状态用例恢复正常执行并通过，没有删断言或增加忽略项。
- nextest 关闭 debug 符号和增量缓存，Makefile/开发入口默认使用该 profile；
  磁盘清理详情见 [清理记录](disk-cleanup.md)。

使用 stable Rust 1.98.1、MSVC 14.51.36231、Windows SDK 10.0.26100.0。
锁定 rustls 0.23.45、rustls-graviola 0.4.0、Graviola 0.4.1；它包含登记的 inline assembly。
后端本机采用结论与限制见 [provider 评估](provider-assessment.md)，不称为密码安全认证。

## 构建与运行矩阵

独立矩阵目录从空目录开始，固定锁文件、工具链与平台，并设置 C 编译器哨兵和链接追踪。
同一目录按配置串行推进；修订仅重验受影响路径，不重复制造完整干净目录。
实际命令、退出码、二进制摘要及导入表见 [matrix-validation.json](matrix-validation.json)，
运行脚本见 [verify-matrix.ps1](verify-matrix.ps1)。

| 范围                 | 验收内容                                                         | 状态   |
| -------------------- | ---------------------------------------------------------------- | ------ |
| B/R/P/PT/G/U nextest | 每行三个真实应用入口构建和链接                                   | 已通过 |
| R/P/PT 生产链接      | infrastructure 单元及数据库/TLS 真实消费者测试入口，非空壳原型   | 已通过 |
| B/G release          | 三个真实应用入口构建；实际 node/FIX 及崩溃重启冒烟               | 已通过 |
| U release            | 并集构建、共享 provider 与不连接服务的运行冒烟                   | 已通过 |
| 全保留目标           | workspace all-features/all-targets，包含库、必要工具及 benchmark | 已通过 |
| 历史转换 release     | 真实原文件转换、结果摘要与资源观测                               | 已通过 |
| 最终来源与加载审查   | 构建脚本、原生源码候选、完整链接输入、导入表、运行模块           | 已通过 |

B/G release 各运行两个既有实际进程用例；U 复用 G harness 对新构建的 U 节点执行本地闭环。
运行节点通过 override 选择本次归档产物，并校验其 SHA-256；不使用旧 release 产物代替。
模块观测只读取这些节点，测试仅清理自己启动的隐藏进程。
结果见 [release-runtime-validation.json](release-runtime-validation.json)。
本轮构建矩阵共 14 项返回 0，B/G/U release 分别执行 2/2/1 个既有实际进程用例。

外部模拟器为显式指定的另一项目已有 exchange.exe，只读使用，未修改或重建。
其 SHA-256 为 CAA0DBD3FAB069EFF400D32FF107CE45B09C52B68ED3275A847A379E948AF290，
每个运行脚本前后复核。测试使用独立目录、端口和固定公开测试凭据。

## 行为与本地工具验收

开发门禁共 20 项，全部返回 0，见 [development-validation.json](development-validation.json)。
覆盖 workspace all-targets check/Clippy、TLS 并集 Clippy、infra 单元、共享 TLS 与数据库本地协议、
真实 gateway TLS/鉴权、B/U 配置、common、P4/P5/P6、B/G FIX、54 项 benchmark 计算及 Redis 恢复编译。
相关命令使用无 debug 符号和增量缓存的 nextest profile。
必要工具、历史兼容与 Rustdoc 的 6 项门禁全部返回 0，见
[tools-validation.json](tools-validation.json)及 [verify-tools.ps1](verify-tools.ps1)。
包含新增依赖策略负向用例、应用 lib/bin、312 项历史兼容用例和 TLS Rustdoc。
Rustdoc 的私有常量链接及 HTML 标记已修正；Windows 共享资源写入冲突通过单任务生成解决。
原失败日志保留，未放宽 lint；文档标记修订只重验文档，没有重复业务矩阵。
本轮开发和工具门禁合计 2,366 次成功测试执行，另有 54 项 benchmark 原计算/断言及
5 个 release 进程用例；跨配置重复执行不称为不同用例，见 [计数清单](test-summary.json)。

common 的 1,793 次成功执行包含两个恢复的 Cache 用例；43 个全局 logger/macros 用例
使用同一刚构建二进制，逐个放入新进程，避免全局 logger 状态互相干扰。
原有两个实验信号重入顺序用例仍忽略，不计为通过；本期 OMS 没有启用该信号消费者。
未来启用前必须处理这两个用例。model 的主动改写文档生成器和 P6 父测试调用的子进程助手
按原用途保留，不作为被跳过的 OMS 行为验收。

P7 中未改变的领域和历史兼容行为可引用原证据；源码逐文件比较记录区分继承证据与本次执行，
不把旧测试数重记为 P8 新执行数。新的依赖图和保留目标仍执行本轮实际构建/链接。
9 个模块的 379 个 Rust 源码/测试文件与 P7 摘要一致，见
[源码比较](prior-source-comparison.json)。

离线策略、schema-check/参数及历史转换共 7 项返回 0，见
[offline-cli-validation.json](offline-cli-validation.json)。
release 转换和校验原始 9,500 条报价共约 5.86 秒，观测峰值工作集 20,643,840 字节。
原文件 SHA-256 为 FE137259BC73EE705C8EC4B84532961C3DCE8F9AB252732BE5F988C6905AC97A，
输出 SHA-256 为 067D046B04E5FF97FD3775D97CA4AA4D34F8920D685BBB9348018D1DE580601E。
本次只记录兼容性及资源观测，不作为容量或部署 RPO/RTO 认证。

## A01–A16 核销

| 编号    | 本期证据或后续边界                                                                                 |
| ------- | -------------------------------------------------------------------------------------------------- |
| A01     | P4 输入的重复/乱序/陈旧与账户门禁回归；实际节点持续接收新鲜 fixture 输入。                         |
| A02–A06 | P5 业务/API、common、应用单元及 P6 重启回归；精确额度、幂等、成交/撤单与受理持久化。               |
| A07–A08 | 本期只保留无 C 构建、链接和本地 TLS/配置；真实认证、schema、事务、重连、消息和投影延期，不记通过。 |
| A09–A10 | P6 恢复及真实节点进程故障；保留不确定占用、原受理结果，恢复后阻断盲重发和新风险。                  |
| A11     | 继承 P6 一致备份、epoch/ID/防重边界并重验恢复；部署 RPO/RTO 和周期指标仍为启用门禁。               |
| A12     | B/G 实际 FIX 模拟进程闭环与 release 冒烟；自动对账/恢复交易仍需完整外部查询证据。                  |
| A13–A14 | P5 API 和真实 gateway TLS/鉴权、事件/快照回归；server TLS 身份不替代应用授权。                     |
| A15     | 14 项矩阵、37 个真实归档产物及 339 份链接追踪通过；生成与运行加载来源已核销。                      |
| A16     | 必要 Rust 生成、构建、测试及本地恢复不要求 Python；DB schema/参数的无服务工具检查通过。            |

## 无 C 审查与限制

源文件、选中 feature、构建脚本及原生候选逐文件摘要保存在本目录 JSON 清单。
来源分类见 [native-source-review.md](native-source-review.md)，最终汇总见
[native-review.json](native-review.json)：37 个真实产物、339 份链接追踪，
编译器哨兵调用为 0，链接输入、产物导入表及运行观测无未分类项。
解析图包含 529 个包，其中 Windows 最终图选中 403 个包和 41 个构建脚本。
编译器哨兵不是唯一依据，必须结合预编译输入、完整链接追踪和可触达加载点。
Rust 工具链、MSVC/SDK 和系统运行库按方案允许；不将这些平台组件误称为第三方业务实现。
仅解析 metadata 中出现但当前图未选择的包另行登记，不能凭锁文件包名判定通过或失败。

首次来源分析保留在 [native-review-first-attempt.json](native-review-first-attempt.json)。
其中 `/IMPLIB` 属于 Rust 宏 DLL 的生成输出，已与输入库区分；CredUI 为 Windows SDK/
系统组件，文件摘要见 [平台证据](credential-ui-platform-files.json)，没有为第三方库新增豁免。
Graviola 的 139 个含 asm 的 Rust 源文件为跨架构清单，不表示所有路径均在本机编译或运行。

最终状态保留 1,176 个源码/配置/fixture 摘要、15 份文档摘要、37 个真实产物及 8 份生成输出。
归档生成协议见 [generated-protocol.json](generated-protocol.json)，
文档检查见 [document-validation.json](document-validation.json)，
最终摘要和本地链接复核见 [final-state-validation.json](final-state-validation.json)。
归档完成后清理专用矩阵编译目录；源码、日志、二进制及生成输出证据保留，见
[磁盘清理记录](disk-cleanup.md)。

真实 PostgreSQL/Redis 服务未启动、未连接。正常节点禁用它们，数据库功能的 A07–A08
和实际采用的普通/TLS、身份、schema、事务、池化、重连、消息模式仍需启用前验证。
自动交易恢复、容量压测、部署 RPO/RTO、mTLS/撤销/轮换、新 CPU/平台均未随本期验收通过。
Linux CI 的更新是静态配置变更，本机没有执行 Linux CI；不宣称跨平台验证。
完整 cargo-audit 在本机不可用，已作选中 TLS 后端的有界公告评估；不宣称全依赖安全审计通过。
没有提交、推送或发布代码。
