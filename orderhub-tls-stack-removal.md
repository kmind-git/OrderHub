# OrderHub 去除第三方 C 实现依赖方案（AWS-LC 路径）

修订日期：2026-10-11。状态：P1 与 P8 已按本期 Windows x64 范围验收；真实 DB 功能仍延期。
阶段证据见 [P1 进度记录](audit/orderhub-simplification-2026-09-27/run-03/P1-progress.md)、
[适配原型](audit/orderhub-simplification-2026-09-28/run-04/P1-adapter-probes.md)和
[本次验收边界调整](audit/orderhub-simplification-2026-09-29/run-05/plan-review.md)。
本文件隶属[项目精简与纯 Rust OMS 方案](orderhub-project-simplification.md)，
对应主方案 P1 技术验证及 P8 最终验收。源码评估可行不等于已在本机运行通过。

## 1. 不可降低的目标

所有最终保留的 OMS 程序、客户端、必要库、生成器、测试和运维工具，不编译、
链接或运行时加载第三方 C/C++ 实现，包括 AWS-LC、ring 和各类 C 压缩/分配器实现。
Redis/PostgreSQL 代码、接口、schema 与现有 Redis TLS 语义必须保留；本期完成无 C 化，
真实服务认证/事务/重连等功能验收延期到启用前。PostgreSQL TLS 适配与 gRPC TLS
按主方案列入目标交付。候选不合格时继续替换或重构，未解决的无 C 路径阻止最终验收。
不得以明文降级、跳过证书校验、C 实现代理或预编译 C 库绕过目标。

允许 Rust 工具链、平台链接器、SDK、操作系统 API 与系统运行库。
外部数据库服务和另一项目的模拟交易所不在本仓库的源码改造范围。
密码后端中的汇编登记为独立组成，不等同于 C 实现，也不称为全部 Rust 源码。
若要求连汇编也排除，Graviola 路线不满足该新增要求，必须重新选型。

验收覆盖实际构建/链接/运行路径，不能仅搜索 `Cargo.lock` 或包名。
`cc` 是 Rust 编写的构建辅助库，`windows-sys` 等也不因名称含 `sys` 就成为 C 实现。
但必须审查它们的实际用途；依赖树没有这些名字也不能证明没有直接编译或加载 C 库。
过渡期锁文件可以包含待裁剪成员的依赖；最终所有保留的受支持路径均须通过。

本文件 C0–C6 是跨阶段工作项，不要求在 P1 一次完成全部应用迁移：

| 时点       | 必须交付                                                               | 不作为该时点门槛                                     |
| ---------- | ---------------------------------------------------------------------- | ---------------------------------------------------- |
| P1         | 关键技术原型、候选适用性评估、可复现补丁和无 C 构建/链接证据、迁移清单 | 全应用迁移、全库历史数据迁移、真实 DB 服务与容量测试 |
| P3–P7      | 按消费者迁移保留代码、工具和测试，完成实际 provider/feature 接入       | 未启用 DB 的新投影功能与实机验收                     |
| P8         | 本期 B/G 行为与全部保留路径（含 R/P/PT/U）的实际无 C 化                | A07–A08 等登记的启用前功能测试                       |
| 能力启用前 | 真实服务、部署证书/身份、schema、事务/重连与投影故障测试               | 未选择的可选模式，例如未使用的 Sentinel              |

待启用 DB 默认关闭，正常入口拒绝未验收的启用配置；专用隔离验证入口显式运行。
本期不要求可用服务实例，也不将协议替身的成功算作真实数据库功能通过。

## 2. 已核实基线及其限制

基于 2026-09-27 本地代码、锁定依赖源码和前述评估：

- 当前 `orderhub-node` 正常/构建图未发现 AWS-LC、ring、cc 或 cmake；
  它尚未接入 infrastructure，也未启用 gRPC TLS。这只是静态基线。
- infrastructure 无条件依赖 cryptography，根 rustls 显式选择 AWS-LC。
  `redis 1.7.0` 的 rustls 依赖关闭默认 feature，TLS 自身没有强制选 AWS-LC，
  可以保留 Redis 客户端并替换 provider。
- 根 `tokio-rustls` 未关闭默认 feature；`reqwest 0.12.28` 的当前 `rustls-tls`
  会引入 ring。必须统一检查 Cargo feature 合并，不只修改一个 rustls 声明。
- SQLx 0.9 当前未启用 TLS；其 TLS 握手源码直接选择 AWS-LC/ring，
  不能仅靠安装新的全局 provider 替换，须适配源码和 feature。
- infrastructure 的开发依赖还引入 live/persistence 等路径及 C 依赖，
  只检验 node 默认构建会遗漏数据库验收依赖。
- BLAKE3 1.8.7 有 `pure` 路径，但 x86 构建脚本在分支前仍探测 C 编译器。
  cryptography 的签名和 testkit 的摘要仍引用 AWS-LC；rcgen 也选择 AWS-LC。
- 本机 x64 已观察到 AES、SSSE3、AVX、AVX2、ADX、BMI2、PCLMULQDQ，
  满足 Graviola 文档的 CPU 要求。这不是 Windows 编译、ABI 或握手验证。

实施前重新记录版本、feature 和源码哈希；不得将以上快照当成后续版本的证明。

## 3. 替代与重构清单

| 依赖/能力                | 首选处理                                    | 关键验证                                         |
| ------------------------ | ------------------------------------------- | ------------------------------------------------ |
| rustls 的 AWS-LC/ring    | rustls + 无 C provider；先验证 Graviola     | 平台、CPU、TLS/证书、算法覆盖和维护风险          |
| Redis TLS                | 保留 redis-rs/TLS，统一 provider            | 本期无 C/本地 TLS；启用前验真实认证、消息和重连  |
| SQLx PostgreSQL TLS      | 小范围 provider 适配补丁                    | 本期 feature/证书/无 C；启用前验真实事务与连接池 |
| tonic/reqwest TLS        | 无默认密码后端的 feature + 统一 provider    | gRPC 双端、必要 HTTPS、根证书来源                |
| SHA-256/HMAC             | RustCrypto `sha2`/`hmac`                    | 固定向量、分块摘要和原输出编码                   |
| RSA 签名                 | 保留接口时使用合格无 C 实现；无消费者则裁剪 | PKCS#8、签名算法、密钥位数和安全性               |
| BLAKE3                   | Rust 实现路径 + 必要构建脚本修正            | 摘要/历史校验兼容，实际编译输入                  |
| zstd/Parquet             | 先随研究框架解耦；必须保留时适配 Rust codec | 旧文件/字典兼容、内存和性能                      |
| mimalloc                 | 移除专用分配器，使用 Rust 默认系统分配器    | 吞吐、延迟和内存回归                             |
| rcgen/证书测试           | 无 C 签名器或固定测试证书样本               | 有效/过期/错名/不受信等场景仍覆盖                |
| PyO3/Python/外部生成器   | 移除绑定；必要工具迁移为 Rust               | 新环境可生成、构建、测试及运维                   |
| libfuzzer-sys 等测试路径 | 迁移必要测试驱动和语料回放为无 C 工具       | 不把旧覆盖要求随工具一起删除                     |

## 4. 按步骤执行

### C0：固定范围与记录构建来源

1. 采用主方案 B/R/P/PT/G/U 矩阵，分别登记本期行为验收和 DB 保留无 C 验收。
   额外工具、保留库和确有消费者的测试/bench/example 同时登记。
1. 对每行收集 normal/build/dev 图、构建脚本、`links`、FFI、预编译对象/库、
   动态加载和外部进程。将 Rust 平台组件与第三方实现逐项区分，不建立宽泛豁免。
1. 记录本地依赖源码和工具链版本，列出每个 C 入口的消费者、替代方法及验收用例。
   尚未实现的 feature/补丁标为计划，不能提前写成已通过命令。

P1 交付：逐项依赖清单和无 C 验证矩阵；每个残留有消费者、处理方法、阶段和验收用例。
P8 核销实际保留路径，退役项提供消费者迁移/无消费者证据，不为退役框架重复建替代实现。

### C1：验证统一密码后端

1. 优先在仓库内隔离原型验证 rustls + rustls-graviola，锁定实际选用版本。
   评估时查阅了 rustls-graviola 0.4.0 和 Graviola 0.4.1 文档；它们尚非本项目已批准后端。
1. 校验本机 Windows x64 构建、运行 ABI 和 CPU 指令，记录汇编输入并执行真实握手。
   CPU 门禁的缺失位分支可用可注入检测结果测试；P1 不要求借用所有不支持型号的实体机器。
   模拟检测输入限于单元测试，正常入口仍读取真实 CPU 能力，不能配置绕过门禁。
   扩展 CPU/系统支持前补相应实机测试，不把注入测试宣称为跨硬件验证。
1. 以首期选择的协议/套件、证书类型和根证书策略验证成功、错名、过期及不受信证书拒绝。
   共用 provider 的底层矩阵只执行一套；各消费者补其证书配置、错误传递和禁止降级测试。
   mTLS、系统根、代理等在实际选择该模式时补测，超时/重连随对应应用接入验证。
1. 对锁定版本记录安全公告、维护状态、算法覆盖、平台约束及已知限制的有界评估，
   给出本地原型是否可采用的明确结论；不把没有外部审计报告本身设成无限等待条件。
   固定向量与互操作通过不能代替此评估，已知且适用的未解决严重问题仍阻塞采用。
   候选不通过则改用经评估的其它无 C provider 或实现 provider 适配并重复测试。
   若必须对密码实现本身进行重构，单列安全设计和审查工作，不能自创算法或降低门槛。

Graviola 是 Rust 与汇编组合，项目仍提示较新；RustCrypto 的 SHA/HMAC 可独立使用。
`rustls-rustcrypto` 项目仍警告不要用于生产，不能直接把它选为默认 TLS 后端。
RustCrypto RSA 的已公开安全警告也须核对具体版本与使用路径，不以纯 Rust 标签替代审查。

P1 交付：有采用结论的选型记录、版本/平台约束、正反向测试、无 C 原型构建/链接证据。
若没有合格候选，P1 保持未完成；不阻塞不依赖该后端的 P2 契约和本地业务实现。
本地原型结论不构成密码库安全认证；生产使用前按实际环境再次评估。

### C2：保留数据库与网络能力并接入 provider

1. 收敛根依赖和所有消费者：rustls/tokio-rustls/hyper-rustls 关闭不需要的默认后端，
   reqwest 0.12 使用匹配根证书策略的 `*-no-provider` feature。
   检查功能并集，防止另一条依赖边重新打开 AWS-LC/ring。
1. 在节点和客户端初始化时设置唯一 provider；初始化失败或配置冲突应明确失败。
   将 cryptography 的 TLS 与独立签名能力解耦，普通 PostgreSQL 不再拉入无关签名实现。
1. 保留 Redis 的 rustls TLS feature 和现有 URI/认证/证书语义，完成保留生产代码的无 C 接入。
   本期只运行无需真实服务的 TLS/配置测试。真实认证、连接管理、重连及使用中的
   Sentinel/Streams 在启用前验证；不改变原 `redis` 为明文专用。
1. 为 SQLx 0.9 新增自选 provider feature 与选择分支，贯通 sqlx/core 及必要宏的转发；
   保留根证书、主机名、客户端身份及错误处理，不只删除原 provider 的 cfg 分支。
   本地补丁置于仓库 `patches/`，保留许可证/版本来源，通过 Cargo 引用，
   不编辑用户 Cargo 缓存。测试 TLS 模式拒绝降级，普通模式必须显式配置。
1. 若 SQLx 补丁无法控制兼容/维护成本，才迁移支持自选 TLS 的 Rust PostgreSQL 驱动，
   保留现有查询、类型、事务、池化及重连的接口和测试，逐项登记迁移差异。
   本期验证编译/链接和本地 TLS；真实行为在启用前验证，禁止以删掉 TLS 结束工作。
1. 使用 tonic 支持的 TLS 配置入口接入统一 provider，验证 server/client、HTTP/2 和鉴权。
   保留显式本机回环明文配置，不将 TLS 配置错误自动回退到明文。

P1 交付：可复现适配原型及生产接入清单，无须先改完最终应用。
P3–P7 接入保留的真实消费者；P8 验 B/G 与必要 HTTPS 的本期行为，
并验证 R/P/PT/U 保留代码的无 C 构建/链接、唯一 provider 和本地配置/TLS。
真实数据库初始化/读写/事务/错误语义列入启用清单，未验证不能宣称无损或可用于业务。
新增 TLS 范围单独标明，不声称原 PostgreSQL 已使用 TLS。

### C3：替换密码原语与哈希构建路径

1. `crates/cryptography/src/signing.rs` 的 HMAC-SHA256 改为 `hmac`/`sha2`，
   `crates/testkit/src/files.rs` 的 SHA-256 改为 Rust 实现；保留十六进制/base64 编码及流式行为。
1. 若 RSA 接口仍有必要消费者，验证合格无 C 实现的 PKCS#8 导入、
   PKCS#1 v1.5/SHA-256 签名、位数限制与 RNG；固定向量和独立验证均通过后替换。
   无必要消费者的接口按主方案裁剪，不为保留通用 API 继续携带 AWS-LC。
1. 保留 Ed25519 已有 Rust 路径；检查 feature 合并和测试依赖，更新真实 provider 版本日志。
1. BLAKE3 先启用 `pure`，审查锁定版 build.rs；必要时小范围补丁使该模式跳过 C 探测/构建，
   不更改摘要算法。P1 用固定向量和受控旧格式样本验证输出；P3–P7 对清点中确需保留的
   历史日志/检查点验证兼容。没有真实旧数据时记录依据，不虚构历史迁移任务。
   功能集成后记录小规模性能变化；规模性能目标不前置到密码原型。

P1 交付替代/退役路线和关键兼容原型；P8 交付必要密码 API 无 C 与实际数据兼容报告。

### C4：处理压缩、分配器和历史数据

1. 随 persistence、回测和研究 fixtures 解耦 zstd-sys；保留 redb 业务 journal。
   P1 先核对必需消费者和数据。确定退役的消费者登记至 P3/P7，
   仅对必须保留且替代可行性未知的格式做 codec 原型。
1. 必须读取的 Zstd 历史文件可评估 `ruzstd`；需要写入时验证它的压缩能力与所需字典，
   不能认定性能、压缩率、边界行为或 Parquet API 与原 zstd 自动等价。
1. 如保留 Parquet，必须实现并测试 codec 适配；如迁移格式，通过 Rust 迁移工具、
   schema 版本和校验报告保护历史数据，不将“不再读取旧文件”当作迁移成功。
1. 清除 mimalloc 的全局分配器配置与依赖，使用 Rust 默认系统分配器，
   若该分配器实际进入首期链路，集成后记录同一小数据集下的延迟、吞吐与内存变化。
   只在将被删除的 CLI/回测中使用时，提供调用图和退役证据即可，不为其做 OMS 压测。

P1 交付消费者/数据清单、处置路线及必要原型；P8 交付无 C 存储/压缩图、
适用的历史数据兼容/迁移证据及资源观测，不要求迁移不存在的数据。

### C5：关闭测试、生成和工具链的旁路

1. 重构 infrastructure/testkit 的开发依赖，不再经研究框架拉入 C；保留业务覆盖。
   第三方库自身的 dev-dependencies 不会自动传递给消费者，不把它们误判为应用依赖。
1. rcgen 使用无 C 签名器或受控测试证书样本，保留全部证书失败场景；
   若必须动态生成，提供 Rust 实现，不以运行 OpenSSL 命令代替。
1. 保留 OrderHub 已有 protox/prost Rust 生成链路；移除不需要的 Cap'n Proto C++
   编译器入口。必须保留的旧格式通过 Rust 编解码/迁移解决，不手改生成文件。
1. Python 绑定、检查、生成和运维按主方案 P3 迁移。若仍保留模糊测试，
   将 libfuzzer-sys 等 C++ 驱动的必要职责迁移为无 C 的驱动、性质测试及语料回放，
   记录原覆盖目标和差距。保留 OMS 使用范围的性质/回归用例，
   随研究功能退役的测试不要求逐条重写；现存 OMS 正确性缺口仍须补齐。
1. 检查 schema、备份/恢复、依赖审计和文档必需命令，禁止必要流程在运行时
   隐式下载或启动 Python/C 实现工具。普通平台 shell 和系统工具不作语言重写要求。

P1 交付入口和迁移清单、未知生成路径的小型验证；P3/P7 完成迁移，P8 验收。
DB 工具本期只验无 C 构建与无服务逻辑，真实 schema/操作行为在启用前验收。

### C6：执行干净构建和行为验收

1. P1 关键原型、P8 最终配置分别执行一次干净构建，固定锁文件、stable 工具链、平台和 feature。
   执行 `--locked` 并记录来源；需要干净证据的运行不复用旧编译产物。
   日常修订允许增量构建；只有构建输入变化或证据缺口才重跑受影响的干净目标。
1. 将 `CC`、`CXX`、`CMAKE`、`NASM` 及目标专用变量指向记录调用并失败的哨兵，
   保留 Rust/MSVC 链接器、系统 SDK。哨兵仅辅助发现调用；直接启动的编译器、
   预编译库、缓存和动态加载还须通过构建日志、进程及链接输入审查覆盖。
   若命中的是已登记的允许汇编路径，单独审查并记录该工具调用；不能将汇编误判为 C 实现。
1. 按主方案第 5 节的 profile/配置覆盖执行真实构建和链接；不同 cfg 均覆盖，
   release 选 B/G/U 及有独立 release 分支的目标。无需重复每行完整双 profile 测试。
   审查 `-vv`、build.rs、静态/动态链接输入和可触达的运行时加载，复用相同依赖的来源证据。
   保留 DB 路径用其生产 crate 的最小链接/本地协议入口取证，不能仅凭空壳原型或 cargo check。
   不执行真实 DB 业务，但仍审查连接路径的加载点；无法审查或隔离的未知加载项阻塞无 C 验收。
1. 本期运行 gRPC 双端、必要 HTTPS、OMS 业务/恢复和 DB 本地 TLS/配置测试；
   共用底层 TLS 用例复用，消费者保留配置/错误/禁止降级覆盖。
   DB 真实认证、事务、连接池、超时重连、投影和消息功能依主方案 A07–A08 在启用前验收。
1. P8 验证无 Python 环境的必要生成、构建、测试和本地备份恢复；DB schema 工具仅验无服务部分。
   记录未运行目标、失败和原因；跳过、仅 cargo check 或只有静态依赖图均不算通过。

证据包括文件哈希、目标/feature、构建脚本、完整命令和退出码、链接输入与平台允许项、
测试/服务/数据版本、结果及未完成项。日志去除凭据，存储路径遵循主方案第 6 节。
格式/Clippy/测试遵循 `CONTRIBUTING.md`；放宽 warnings 的诊断运行不能算正式门禁通过。
执行和清理按配置串行安排，保存证据后可删除不用的 debug 产物；不为归档重复构建已验证二进制。

## 5. 完成条件

P1 技术退出清单（2026-09-30 已按本阶段范围验收；逐项证据见
`audit/orderhub-simplification-2026-09-30/run-08/P1-acceptance.md`）：

- [x] C0：最终保留/退役路径均有消费者、处理方式、实施阶段和验收项。
- [x] C1：锁定候选的本机适用性评估、正反向 TLS、无 C 构建/链接证据通过。
- [x] C2：SQLx/Redis/gRPC/必要 HTTPS 原型与补丁可复现，生产接入清单明确。
  Redis 保留生产路径的无 C 接入与本地 TLS 探针已完成，见
  `audit/orderhub-simplification-2026-09-29/run-07/Redis-no-c-integration.md`；
  P1 时 PostgreSQL 普通连接的生产路径已通过无服务链接探针，SQLx TLS 补丁
  当时仅在隔离原型；最终应用接入与链接已由下列 P8 证据核销。
- [x] C3–C5：保留路径的技术不确定性已用原型消除，确定退役路径有迁移依据。
- [x] C6：上述原型证据可追溯；失败、限制与延期项明确。

P8 本期最终清单：

- [x] C0：实际保留图、必要目标及退役消费者核销。
- [x] C1–C2：本期 TLS 应用行为及 R/P/PT/U 保留生产代码的无 C 接入通过。
- [x] C3：必要签名/摘要无 C，存在的必需历史校验数据仍兼容。
- [x] C4：压缩/分配器迁移与适用的数据兼容、资源观测完成。
- [x] C5：必要测试/生成/工具入口无 C、无 Python，OMS 覆盖保留。
- [x] C6：按主方案分层矩阵完成构建、链接、运行和本期行为证据。

实际构建、链接、运行及延期边界见
[P8 验收记录](audit/orderhub-simplification-2026-10-10/run-18/P8-acceptance.md)。

DB 功能启用清单独立跟踪：真实服务 A07–A08 和所选 URI/认证/TLS/消息模式通过后，
才可移除未验收配置门禁；P1 或 P8 结项均不会自动批准数据库功能启用。
既有报告保留原日期与原结论，新范围以主方案及 2026-09-29 调整记录为准。

## 6. 评估来源

- [Graviola 文档](https://docs.rs/graviola/latest/graviola/)：无 C 构建、汇编及 CPU 边界。
- [rustls-graviola](https://docs.rs/rustls-graviola/latest/rustls_graviola/)：provider 接口。
- [RustCrypto Hashes](https://github.com/RustCrypto/hashes)与
  [MACs](https://github.com/RustCrypto/MACs)：Rust SHA/HMAC 实现。
- [RustCrypto RSA](https://github.com/RustCrypto/RSA)：签名候选及公开安全警告。
- [rustls-rustcrypto](https://github.com/RustCrypto/rustls-rustcrypto)：使用限制。
- [ruzstd](https://docs.rs/ruzstd/latest/ruzstd/)：Rust Zstd 能力与限制。
- 本地锁定源码：redis 1.7.0 的 Cargo feature 和 TLS builder、SQLx 0.9.0 的
  `sqlx-core/src/net/tls/tls_rustls.rs`、tonic 0.14.6 的 TLS 配置入口、
  reqwest 0.12.28 的 feature、BLAKE3 1.8.7 的 build.rs、rcgen 0.14.10 的签名接口。

外部来源为评估依据；实施时锁定实际版本并再次核对，不能把浮动文档链接当作构建锁定。
