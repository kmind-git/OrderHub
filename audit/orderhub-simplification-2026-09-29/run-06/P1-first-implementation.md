# P1 第一批实施：依赖隔离与 TLS CPU 门禁

日期：2026-09-29。目标平台：`x86_64-pc-windows-msvc`，Rust 1.98.1 stable。
范围：P1 C0/C3 的依赖处置和 C1 的 CPU 失败分支；P1、C0–C6 均未因本批变更自动结项。

## 改动及依据

- `nautilus-infrastructure` 原来无条件依赖 `nautilus-cryptography`。
  仓库源码中该 crate 的引用仅在受 `redis` feature 保护的 `redis/cache.rs` 和
  `redis/msgbus.rs`。现将密码依赖设为可选，并由 `redis` feature 选入。
  PostgreSQL 单独构建不再因这条边编译 AWS-LC；Redis 仍使用既有 provider。
- 根 `Cargo.toml` 的 `rcgen` 工作区依赖没有找到直接 crate 消费者，已移除声明。
  没有修改证书验证实现或已有测试 fixture。
- 更新 infrastructure README，说明各 feature 对密码依赖的选择。
- 隔离 TLS 原型的正常 CPU 检测仍读取本机指令。检测结果统一进入验证函数，
  新增单元测试逐项模拟七种必需指令缺失，确认首个缺失项被拒绝。
  正常入口没有测试开关，也没有修改生产密码后端。

修改前 `cargo tree --locked --offline -p nautilus-infrastructure --no-default-features
--features postgres --target x86_64-pc-windows-msvc -e normal,build -i aws-lc-sys`
显示 `aws-lc-sys -> aws-lc-rs -> nautilus-cryptography -> nautilus-infrastructure`。
这说明原 PostgreSQL 单独路径含一条无业务必要的 C 实现依赖边。

## 本批验证

| 检查 | 结果及边界 |
| --- | --- |
| `cargo metadata --locked --offline --no-deps --filter-platform x86_64-pc-windows-msvc --format-version 1` | 退出码 0；锁文件与 manifest 一致 |
| PostgreSQL 单独 feature 的 Windows normal/build 依赖树 | 退出码 0；不再选择 `nautilus-cryptography`、AWS-LC、ring、zstd、mimalloc 或 `rcgen` |
| Redis 单独 feature 的 Windows normal/build 依赖树 | 退出码 0；仍选入 Redis、`nautilus-cryptography` 与 AWS-LC，未误删 Redis TLS 依赖 |
| `cargo check --locked --offline -p nautilus-infrastructure --no-default-features --features postgres --lib` | 退出码 0；稳定版实际编译通过，不需要数据库服务 |
| `cargo clippy --locked --offline -p nautilus-infrastructure --no-default-features --features postgres --lib -- -D warnings` | 退出码 0 |
| `cargo check --locked --offline -p nautilus-infrastructure --no-default-features --features redis --lib` | 退出码 0；稳定版编译通过，未连接 Redis 服务 |
| `cargo fmt --all -- --check`、`git diff --check` | 退出码 0 |
| 隔离 TLS 原型的新目标目录 + 编译器哨兵，`cargo test --offline --locked --manifest-path prototypes/orderhub-no-c-tls/Cargo.toml --lib` | 退出码 0；9/9 通过，包括当前机器检测、七项缺失模拟及 TLS/证书测试 |
| 隔离 TLS 原型 `cargo clippy --offline --locked --manifest-path prototypes/orderhub-no-c-tls/Cargo.toml --all-targets -- -D warnings` 与 `cargo fmt --manifest-path prototypes/orderhub-no-c-tls/Cargo.toml -- --check` | 退出码均为 0 |

原型测试使用先前不存在的 `target/p1-cpu-probe` 目录，`CC`、`CXX`、`CMAKE`、`NASM`
及目标专用 `CC`/`CXX` 均指向会失败的仓库内 `compiler-sentinel.cmd`；未触发哨兵。
这验证当前原型构建未调用这些编译工具，不代替所有链接输入或运行时加载审查。
验证后核实无 Cargo/Rust 编译进程，再删除本次生成的 `target/debug` 和
`target/p1-cpu-probe`；两个路径均在仓库内，源码、锁文件与本记录保留。

源文件/输入 SHA-256（本次验证的文件状态）：

| 文件 | SHA-256 |
| --- | --- |
| `Cargo.toml` | `933E2DF8506E881EFD5FF598C594DD88E5BD2E8168ADF9623FFA2081D7D705DE` |
| `Cargo.lock` | `377D37DC55F099869576D15CF0402C7197A57EF6FB374F3971B7A8A931BE6F75` |
| `crates/infrastructure/Cargo.toml` | `7280AF0D61420D6C6AC8F5694F026800E71E164E60D9122B239896ACB7C54105` |
| `crates/infrastructure/README.md` | `9E2F2B035127311F99EFF89769F2AF3F1D4819D1D1BE90DE707DE1324867E326` |
| `prototypes/orderhub-no-c-tls/src/lib.rs` | `84608EAA4EC4CA8656A4B9BA3E5D4BC10459B6D277E4A0E227DB4D2A97A778EC` |
| `prototypes/orderhub-no-c-tls/README.md` | `44943D2B6973DEB7ED61663217874D7236895EAFB8EB60FACB1F6AC810550189` |
| `prototypes/orderhub-no-c-tls/Cargo.lock` | `9638B7D14388C81AC37ED1D4BB01DDCC86FEDA8F57AC106889AAC86DFBF51513` |

## 尚未证明的事项

- 本次 PostgreSQL 配置未启用 TLS。SQLx provider 补丁仍只在隔离原型中，
  不能从本批普通连接路径推断 PostgreSQL TLS 已无 C 化。
- Redis 与 `nautilus-cryptography` 仍选择 AWS-LC；保留的测试/工具路径也尚待 C0–C5 核销。
- `cargo check`/Clippy 和 Cargo 依赖树不构成最终链接、运行时加载或完整 C6 证据。
  未运行真实 PostgreSQL/Redis 认证、事务、消息或重连；按最新方案在启用前验收。
- CPU 失败测试模拟检测结果，并非在不支持的实体机器上执行；平台支持声明限于
  当前 Windows x64 机器。Graviola [维护方 README](https://github.com/ctz/graviola)
  仍提示项目较新并列出 CPU 限制；本次未批准它作为生产密码后端。
- P1 尚需锁定候选密码后端的适用性/安全评估、剩余依赖处置及原型链接证据。
  最终应用集成和全部保留配置的无 C 验收属于 P3–P8。

本次未修改业务受理、风控、交易、数据库连接或 TLS 运行时代码，未提交或推送。
