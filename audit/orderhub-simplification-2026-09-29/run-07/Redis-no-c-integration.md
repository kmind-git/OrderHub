# P1 Redis 保留路径无 C 接入

日期：2026-09-29。平台：Windows x64，Rust stable 1.98.1。范围是
`nautilus-infrastructure` 的 `redis` feature 与它的本地 TLS 链接探针；
未连接真实 Redis 实例。这里的“无 C”指保留业务路径不选入第三方 C/C++
实现，系统 API、编译器和链接器不在排除范围内。

## 实施

- Redis feature 改为选择固定版本 `rustls-graviola = 0.4.0`（锁文件中
  `graviola = 0.4.1`），移除 Redis 对 `nautilus-cryptography` 的依赖边。
  原 crate 的 AWS-LC provider 仍留给其他尚未迁移的消费者；没有将其状态
  误记为全应用无 C。
- 缓存和消息总线不再调用 AWS-LC provider。所有 Redis 连接在共同入口
  `create_redis_connection` 中按 `ssl` 显式选择 `rediss://` 与 Graviola；
  明文配置不初始化 TLS。消息总线构造器会对 TLS 的 CPU/全局 provider
  冲突立即返回错误。失败不会回退到明文。
- Windows/Linux x86-64 检查 Graviola 所需七项 CPU 指令；aarch64 检查
  AES、SHA2、PMULL、NEON；其他架构明确拒绝 TLS。缺失检测已增加纯函数
  单元测试，当前批次未运行该单测；先前隔离 TLS 原型已覆盖七项缺失注入。
  保留原证书、主机名及 Redis URI 处理语义。
- 本地探针现在依赖生产 `nautilus-infrastructure`，先由生产连接入口
  初始化 provider 并拒绝不受信任证书，再验证受信任证书的 TLS/RESP、
  错误主机名拒绝以及显式明文 URI。探针是本机临时 TLS/RESP 夹具，
  不模拟 Redis 服务行为。

## 验证

| 检查 | 结果 |
| --- | --- |
| `cargo build --locked --offline -p nautilus-infrastructure --no-default-features --features redis --lib` | 通过；稳定版实际编译生产 Redis 库 |
| 同一 feature 的 Windows `cargo tree --locked --offline ... -e normal,build` | 仅见 Graviola；未见 AWS-LC、ring、OpenSSL、zstd、`cc`、CMake 或 bindgen |
| 本地探针同样的 normal/build 依赖图 | 未见上述第三方 C 构建链；生成并链接了 Windows 测试程序 |
| `cargo test --locked --offline --manifest-path prototypes/orderhub-redis-tls/Cargo.toml --lib` | 1/1 通过；生产 TLS 拒绝无根证书、本地 TLS/RESP 与主机名正反例、明文显式配置 |
| `cargo clippy --locked --offline -p nautilus-infrastructure --no-default-features --features redis --lib -- -D warnings` | 通过 |
| `cargo clippy --offline --locked --manifest-path prototypes/orderhub-redis-tls/Cargo.toml --all-targets -- -D warnings` | 通过 |
| `cargo fmt --package nautilus-infrastructure -- --check` 与探针 `cargo fmt ... --check`、`git diff --check` | 通过 |

本次直接验证只覆盖当前 Windows x64、所列 feature 和 normal/build 依赖图。
链接探针提供真实 Rust 二进制运行证据；没有对全部应用的链接输入及
运行时库做最终 C6 核销。Rust/汇编组合的 Graviola 候选仍需安全和维护
适用性评估，尚未据此批准实盘使用。真实 Redis 认证、连接管理、重连、
Sentinel 和 Streams 按方案保留至功能启用前，用真实服务验证；
这些项目不阻塞本期无 C 实施。P1 整体仍未结项，SQLx、gRPC、必要 HTTPS
及其他保留路径依各自清单继续。

验证完成且确认无 Cargo/Rust 编译进程后，已核实目录位于仓库内并删除
`target/debug` 与 `prototypes/orderhub-redis-tls/target/debug`。删除后
D 盘可用空间约 60.8 GB；源码、锁文件和本记录保留。

关键输入 SHA-256：

| 文件 | SHA-256 |
| --- | --- |
| `Cargo.lock` | `07E0043BA73C05D1045423A96AB1A7F2E59935CE0E22D6E0E2E2BB97BA4A145C` |
| `crates/infrastructure/Cargo.toml` | `AE16C1BAEE2F644B9C54AD40FCFACF8510BBFCA236CAC6DD9CD343835A4158B3` |
| `crates/infrastructure/src/redis/mod.rs` | `C0A3A2039D89F7662203B7B9B902675501F86082FD35C3E7CAE2E8EDBFA23A52` |
| `crates/infrastructure/src/redis/cache.rs` | `C15A1773BE50CB3185F8104F6B1B24BF6F1210F5407E98E9646B084781849552` |
| `crates/infrastructure/src/redis/msgbus.rs` | `02B99C58E963452112F07E4DE26E54F6C0823D21120021B07CD8A572EB738B4D` |
| `prototypes/orderhub-redis-tls/Cargo.lock` | `1908A0A713ED8ACB5067984D3FB0816CC98717B76786F2776757BD77D4E79CF3` |
| `prototypes/orderhub-redis-tls/src/lib.rs` | `0E17CBEC597A65C395935AE48A4EEC2FC0543404412D6DB8FF33865BDF2D0B1B` |

未提交或推送代码。
