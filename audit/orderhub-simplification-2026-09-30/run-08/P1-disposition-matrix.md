# P1 残留路径、实施阶段与验收项

日期：2026-09-30。范围：Windows x64、stable 1.98.1、当前仓库的正常/构建/开发选择。
本表是 P1 的**路径处置决定**，不表示 P3–P8 的迁移已完成。当前依赖图的具体选择
见 run-03 `*-normal-build-tree.txt` 和 run-07 Redis 记录；run-02 的
`native-candidate-workspace-consumers.csv` 是宽松的反向可达清单，不可当成
某个目标已实际链接的证明。当前根工作区 `postgres` 单独路径及 `redis` 单独
路径未选入已知第三方 C/C++ 候选；其它 crate/feature 仍会选入。

| C 项/残留 | 当前消费者和数据 | 处理方式与负责阶段 | 验收项 |
| --- | --- | --- | --- |
| C1 TLS provider | Redis 生产入口已选 Graviola；SQLx、reqwest、tonic 仍为隔离原型。Windows x64 有 CPU 指令门槛。 | P1 锁定组合并作有界安全决定；P3–P7 把生产消费者接入统一 provider；P8 重新核对最终 feature 并集。 | P1 正反向证书、缺失 CPU 位、构建/链接；P8 最终应用 TLS 错配、证书和唯一 provider 失败路径。 |
| C2 Redis | `nautilus-infrastructure` 的缓存和消息总线，共享 `create_redis_connection`；`redis` feature 已在生产 crate 去除 AWS-LC 依赖边。 | P1 保留此实现；P3–P7 接入最终节点并保持 `rediss://` 和显式普通 URI。真实服务行为在启用前验证。 | P1 生产库构建、正常/构建依赖图、本地 TLS/RESP 和链接；P8 最终保留配置的无 C 构建/链接；启用前认证、管理器、Sentinel/Streams、重连。 |
| C2 PostgreSQL/SQLx | 根 `sqlx = 0.9.0` 当前仅有普通 PostgreSQL feature；本地四 crate 补丁只被 SQLx TLS 原型引用。生产 `nautilus-infrastructure` 的 `postgres` feature 另有无服务、无 C 链接探针。 | P3–P7 将自选 provider feature/补丁接入保留的生产 SQLx 边，明确 TLS `VerifyFull` 与根证书；本期只做无服务路径和补丁复现。 | P1 有锁 TLS 原型成功/错名/拒绝 TLS/显式普通模式，生产普通路径无 C 图/链接；P8 生产 TLS feature 无 C 图/链接、配置不降级；启用前真实 DB 认证、查询、事务、池化和重连。 |
| C2 gRPC/HTTPS | tonic 双端与 reqwest 必要 HTTPS 是隔离原型；`orderhub-proto` 当前还未启用 gRPC TLS。 | P3–P7 在实际保留的 server/client 和 HTTPS 请求路径接入相同 provider；保持明确的本机明文配置。 | P1 本地 HTTP/2 gRPC、HTTPS 成功与错名拒绝、无 C 链接；P8 最终应用授权、错误传播、证书和配置，必要时系统根/mTLS/代理。 |
| C3 HMAC/SHA | `cryptography/signing.rs` HMAC 和 `testkit/files.rs` SHA-256 已用 RustCrypto；cryptography 的 TLS 与 RSA 仍带 AWS-LC。 | P3–P7 将 TLS 依赖移出密码工具或替换，保证普通业务不重新选入 AWS-LC。 | 固定摘要/签名向量、流式跨缓冲区、最终依赖与链接输入。 |
| C3 RSA/Ed25519 | RSA 签名及 Python binding 在 cryptography；未找到 OrderHub Rust RSA 调用点，外部 Python 消费者尚未确认。Ed25519 已有 Rust 路径。 | 按首期 OMS 范围规划 P7 退役 RSA 绑定/API；P3 先核对外部 Python 消费者并完成迁移。若发现必须保留的 RSA 消费者，变更路线前评审无 C 实现并验证 PKCS#8/PKCS#1 v1.5/SHA-256、密钥位数与 RNG。 | P8 提供无消费者/迁移证据且不再选入 AWS-LC；路线变更时补固定向量与独立验签。不得凭当前无 Rust 调用就删 Python API。 |
| C3 BLAKE3 | journal 等路径使用锁定 `blake3 1.8.7`；`pure` 模式的本地 build.rs 小补丁已原型验证。 | P3–P7 将 `pure` 应用于保留消费者，核对真实历史样本和成本。 | P1 固定向量及无 C 构建；P8 数据兼容、最终 build.rs/链接输入和小规模资源观测。 |
| C4 Parquet/Zstd | `nautilus-persistence` 的 Parquet/Arrow/DataFusion 与 `nautilus-testkit` 的 Parquet 选择 `zstd-sys`；`nautilus-event-store` 的可选 `persistence` feature 仍调用 Parquet 回放。`test_data/nautilus/arrow` 有 Parquet fixtures，persistence 中有旧 catalog 迁移测试。仓库源码搜索未发现 OrderHub redb journal 使用 Parquet。 | P3/P7 先确定目录/回放/外部数据消费者、保护历史文件，迁移需保留数据至无 C 格式；若必须继续读写 Zstd/Parquet，再作 Rust codec 及兼容原型。不要仅因首期 OMS 不用而删除历史读取能力。 | 真实文件清单、schema/记录数/校验和迁移报告或保留格式的 codec 读写；P8 最终 normal/build/dev 图不选入 zstd-sys。 |
| C4 mimalloc | `nautilus-cli` 直接使用；回测 feature 可选；当前 OrderHub 默认 normal/build 图未选入。 | P7 迁移必要 CLI 命令/消费者后退役或改用系统分配器；若进入保留链路，记录同数据集资源变化。 | CLI 消费者迁移、最终图/链接；只对实际保留链路作资源比较。 |
| C5 testkit 开发路径 | `testkit/files.rs` 仍初始化 AWS-LC provider 以下载数据；Parquet/Zstd 和其它传递依赖带 ring。infrastructure 的 dev 边也会扩大图。 | P3/P7 用统一 provider 或固定离线 fixtures，保留 OMS 必要测试，裁剪研究数据入口；独立核对 dev 图。 | 必要测试不缩减证书负例、哈希/数据断言；P8 PT 目标的无 C 构建/链接。 |
| C5 rcgen/证书 | 根工作区未使用的 rcgen 声明已移除；隔离原型有受控固定证书 fixture 和生成脚本。 | P3/P7 保留 fixture 来源与更新步骤；若新增动态证书需求，选无 C 签名器。 | 成功、过期、错名、不受信测试持续可运行；证书生成输入可追溯。 |
| C5 Cap'n Proto/protox | `nautilus-serialization` 的 `capnpc` 在可选 `capnp` feature；旧安装脚本下载 C++ 编译器。OrderHub proto 使用 Rust protox/prost。 | P3/P7 查旧格式消费者并迁移/退役 C++ 生成入口；不要手改生成代码。 | 保留格式 roundtrip/迁移与生成漂移检查；P8 生成链路无第三方 C/C++。 |
| C5 fuzz/Python | `nautilus-live` 可选 `libfuzzer-sys`；Python 绑定与运维工具仍在广义项目范围。 | P3/P7 将 OMS 必要语料/性质测试迁移到无 C 驱动，必要构建/运维入口改为 Rust。 | OMS 覆盖职责和语料回放记录；P8 无 Python 必需流程及 U/PT 图。 |
| C0/C6 SQLite 和其它构建旁路 | 宽松元数据曾列 `libsqlite3-sys`；当前 Windows 工作区 normal/build 和 dev 选择未见它。其它 feature/target 可能改变选择。 | P3–P7 每次保留消费者/feature 变更后重算实际选择；P8 覆盖 B/R/P/PT/G/U，审查 FFI、静态/动态输入与运行时加载。 | 选定目标完整有锁命令、构建日志、链接输入和运行时库；不以宽松元数据或 `cargo check` 单独核销。 |

生产接入顺序：先固定根 feature 并集和唯一 provider 初始化，再逐条迁移 Redis、
PostgreSQL、gRPC、HTTPS 使用点及错误处理，最后清理旧 crypto/test/tool 边。
每条边切换后重算依赖图并运行其本地证书/配置用例。任何消费者尚未迁移时保留
对应源/API 和历史数据；P8 才宣称最终应用无 C。DB/Redis 的真实服务验证
按用户已定范围留到**启用前**，不作为本期 P1 退出项。
