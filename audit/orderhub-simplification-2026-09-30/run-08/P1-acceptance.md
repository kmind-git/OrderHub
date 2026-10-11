# P1 无 C 技术路径验收

日期：2026-09-30。平台：`x86_64-pc-windows-msvc`，仓库钉住的 stable Rust
1.98.1；基准提交 `479d8674d80af212044731ef43c9a24db30aa40f` 加本地改动。
此文按最新版主方案 P1 退出要求判断**技术原型与处置路线**，不是 P8 最终
应用、生产部署或真实 PostgreSQL/Redis 服务验收。关键输入的 SHA-256 在
`source-hashes.txt`，完整构建输出和退出码在本目录。前批证据见 run-03、
run-04、run-06、run-07；本轮只重验受影响或尚缺链接证据的路径。
**结论：P1 五项技术退出条件通过，进入按消费者推进的 P3–P8 集成与最终验收。**

## P1 退出项

| 项 | 证据与判定 |
| --- | --- |
| C0 消费者与处置 | `P1-disposition-matrix.md` 对 TLS、Redis、SQLx、gRPC、HTTPS、密码原语、RSA/Python、BLAKE3、Parquet/Zstd、mimalloc、testkit、证书、Cap'n Proto、fuzz 和广义工具入口记录当前消费者/数据、处理方式、阶段和验收。最终 26 模块处置见主方案第 3 节。此处核销的是 P1 路线，不是消费者迁移完成。 |
| C1 候选适用性 | `P1-provider-selection.md` 对锁定版本、Windows CPU 指令、算法、维护/公告和适用边界给出采用决定。基础 rustls 原型本轮 9/9 通过，涵盖 TLS 1.2/1.3、RSA/ECDSA、有效/错名/过期/不受信证书和七项 CPU 缺失模拟；真实机器的 CPU 检测也运行。 |
| C2 适配与补丁 | SQLx 0.9 本地四 crate 补丁保留上游来源和许可证，feature 转发已在有锁原型实际编译；SQLx、gRPC、HTTPS 各 1/1；Redis 使用**生产** infrastructure 入口的本地 TLS/RESP 探针 1/1。成功及失败证书、协议拒绝、禁止降级按各原型覆盖。`P1-disposition-matrix.md` 列出最终生产接入顺序和每条功能边的 P8/启用前验收。 |
| C3–C5 技术不确定性 | HMAC/SHA 已改 RustCrypto 并有固定向量；BLAKE3 `pure` 的本地小补丁、固定向量及无 C 哨兵构建见 run-04 `P1-blake3-pure.md`。RSA/Python、旧 Parquet/Zstd、通用 CLI、Cap'n Proto、fuzz 和研究测试路径按消费者迁移后退役；若确认历史格式必须保留，再做兼容 codec 原型，P1 不虚构该需求。rcgen 根声明已移除，固定证书 fixture 覆盖反例。 |
| C6 有锁构建/链接 | 本轮五条 TLS 原型均使用新目标目录、`--locked --offline`、`-vv`、失败式 C/C++ 编译器哨兵；各命令退出 0。`native-link-arguments.txt` 未发现第三方 C 静态库显式链接，Redis 仅见 Windows 系统库名；`linked-exe-dependents.txt` 的 PE 导入表仅含 Windows 系统 DLL 与 MSVC 运行库。仍须 P8 对最终保留图、release 分支、动态加载及其它平台重新核销。 |

## 本轮命令和结果

以下命令均从仓库根目录执行。共享 `CARGO_TARGET_DIR` 指向本目录的**新**
`target`；`CC`、`CXX`、`CMAKE`、`NASM` 和目标专用 `CC`/`CXX` 指向
`prototypes/orderhub-no-c-tls/compiler-sentinel.cmd`。`RUSTFLAGS` 为
`-C link-arg=/VERBOSE:LIB -A linker_messages`；后者只允许 Windows MSVC
链接器的信息消息，和仓库 `.cargo/config.toml` 的既有例外一致。
SQLx 的本轮首试在新目录中部分编译后失败，重试复用该目录；run-04
另有完整的干净目录 SQLx 成功记录。其它本轮目标均从该新目标目录构建。

| 命令主要部分 | 日志/退出码 | 结果 |
| --- | --- | --- |
| `cargo test --locked --offline --manifest-path prototypes/orderhub-sqlx-tls/Cargo.toml --lib -vv` | `sqlx-link-retry.log` / `.exit` | 0；1/1 |
| 同命令，基础 `orderhub-no-c-tls` | `orderhub-no-c-tls-link.log` / `.exit` | 0；9/9 |
| 同命令，`orderhub-grpc-tls` | `orderhub-grpc-tls-link.log` / `.exit` | 0；1/1 |
| 同命令，`orderhub-https-tls` | `orderhub-https-tls-link.log` / `.exit` | 0；1/1 |
| 同命令，`orderhub-redis-tls` | `orderhub-redis-tls-link.log` / `.exit` | 0；1/1，链接生产 Redis crate |
| `cargo tree --locked --offline ... -p nautilus-infrastructure --no-default-features --features postgres --target x86_64-pc-windows-msvc -e normal,build` | `postgres-production-tree.txt` / `.exit` | 0；未选入已知第三方 C/C++ 候选 |
| `cargo run --locked --offline --manifest-path prototypes/orderhub-postgres-no-c/Cargo.toml -vv` | `postgres-probe-link.log` / `.exit` | 0；生产 PostgreSQL crate 独立有锁链接并运行，无需 DB 服务 |
| `cargo fmt --manifest-path prototypes/orderhub-postgres-no-c/Cargo.toml -- --check`、`cargo clippy --locked --offline --manifest-path prototypes/orderhub-postgres-no-c/Cargo.toml --all-targets -- -D warnings`、`git diff --check` | `final-checks.txt`、`postgres-probe-clippy.log` / `.exit` | 均 0 |

首次 SQLx `/VERBOSE:LIB` 构建因直接设置 `RUSTFLAGS` 覆盖了项目原有
`-A linker_messages` 而被 `build.warnings=deny` 中断（`sqlx-link.log`）。
按已有 Windows 例外重跑后通过，保留首次失败供复核。随后直接运行
`cargo test -p nautilus-infrastructure --no-default-features --features postgres --lib -- --list`
时，**开发依赖**选入 `zstd-sys`，失败式编译哨兵以 99 阻止 C 编译，退出
101（`postgres-production-link.log` / `.exit`）。这是已登记的 C5/PT
路径缺口，不把它写成 PostgreSQL 正常/构建路径失败，也不以普通测试构建
代表后者。生产路径的独立无服务链接探针位于
`prototypes/orderhub-postgres-no-c`；其 normal/build 图在 `postgres-probe-tree.txt`
中未发现已知第三方 C 候选，编译器哨兵未触发，构建/运行退出 0。
该探针的 PE 导入表 `postgres-probe-dependents.txt` 仅见 Windows 系统 DLL
与 MSVC 运行库，`postgres-probe-link.log` 未见第三方 C 候选编译或显式
原生库输入。它只验证当前生产普通连接依赖边的链接，不证明 SQLx 补丁已
进入生产 feature。

## 延期边界与后续实施

- 根应用仍未把 SQLx 补丁、自选 provider、gRPC/HTTPS TLS 接入最终服务；
  Redis 已接入保留生产入口。P3–P7 按处置表迁移，P8 重验最终图与运行时。
- SQLx 当前生产 `postgres` feature 只证明现有普通连接路径的无 C 选择。
  新增 TLS 必须启用本地补丁和 `VerifyFull` 等明确策略，并重做最终链接与
  证书/禁止降级测试。不得把无 C 普通路径误报成 PostgreSQL TLS 已上线。
- 真实 PostgreSQL/Redis 认证、事务、池、Sentinel/Streams、重连及恢复
  只在启用这些服务前验证；本期未连服务，不宣称这些行为已通过。
- Graviola 是当前 Windows x64 候选的**有界采用决定**；维护方提示其较新，
  其它 CPU、系统、混合密钥交换互操作和实盘环境仍需对应验收。
- 原型的成功不等于 P8 全应用无 C。`nautilus-testkit`、cryptography、
  persistence 等仍有 AWS-LC、ring、Zstd 或工具链旁路；按矩阵迁移后复核。

无提交、推送、远端问题或 PR 操作。
