# 验证记录

完成时间：2026-09-22，时区 Asia/Singapore。审查基线为 develop / `79705a0553af78c7135636061fb14390f54ca869`。所有命令从 `D:/projects/zcodeworkspace/nautilus_trader` 执行；没有连接真实交易账户。

## 结果

| 验证 | 结果 | 证据 |
|---|---|---|
| 全工作区 Rust 格式检查 | exit 0；stable rustfmt 忽略四项 nightly-only 配置 | [format-check.log](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/format-check.log) |
| OrderHub 现有测试，单独重跑 | **26 passed，0 failed，1 filtered out**；exit 0 | [cargo-tests-isolated.log](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/cargo-tests-isolated.log) |
| 审查专用探针 | **13 passed**，其中 9 个缺陷探针、4 个随源码包含的原有 FIX 单测；exit 0 | [probe-results.log](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/probe-results.log) |
| Redis 原函数定向复现 | 2 个断言成立：短密码原样保留、多字节密码 panic；exit 0 | [redis-probe-results.log](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/redis-probe-results.log) |
| 源码完整性 | 36 个审查范围文件的 SHA256 与开始时一致，Cargo.lock 原有改动保留 | [integrity-check.json](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/integrity-check.json) |

**缺陷探针的“通过”表示成功复现当前错误行为，不表示功能通过生产验收。** 这些探针位于审查目录，没有加入正式测试套件，也没有修改生产代码以配合探针。

## 现有测试命令与计数

环境为 Windows / x86_64-pc-windows-msvc，rustc 1.98.1、cargo 1.98.1。使用离线、锁定依赖的 release 构建。测试临时数据放在审查目录的 tmp 子目录。

```powershell
$env:TEMP = Join-Path (Get-Location) 'audit/production-review-2026-09-21/tmp'
$env:TMP = $env:TEMP
cargo test --release --locked --offline --no-fail-fast -p orderhub-gateway -p orderhub-proto -p orderhub-node -- --skip m2_local_e2e_full_chain_fill --test-threads=1 --nocapture
cargo fmt --all -- --check
```

| 测试目标 | 通过数 | 备注 |
|---|---:|---|
| orderhub-gateway lib | 5 | 日志 key 与本地 FIX 编解码 |
| m0_validation | 6 | 执行队列、拒单、幂等、日志 reopen 与写盘 |
| m1_acceptance | 3 | Sandbox 成交、撤单及日志吞吐 |
| m1_grpc | 1 | 当前 gRPC 正常闭环 |
| m1_validation | 5 | 额度对象、补流与 group commit |
| m2_validation | 6 | 授权对象、额度对象、readiness 与日志恢复 |
| m2_local_e2e | 0 | 1 个用例明确过滤，原因见下文 |
| node/client/proto 与 doctests | 0 | 对应目标运行完成，无测试用例 |

第一次运行在 m1_acceptance 的吞吐断言失败后停止：1,968 writes / 1.381s，约 **1,425 writes/s**，门槛为 2,000。当时审查探针正在进行耗时 LTO 编译，存在资源竞争。保留 [cargo-tests-initial.log](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/cargo-tests-initial.log)；没有改动断言或门槛。

待探针编译结束后，上述单独重跑通过：同一吞吐用例 **1,968 writes / 0.270s，约 7,278 writes/s**；group-commit 用例 **2,016 admissions / 338,890us，约 5,949 admissions/s**。这是 Windows 本机、直接调用持久化 worker 的短时测试，不能外推为 Linux 实盘端到端吞吐或延迟保证。首次失败的具体原因没有通过受控实验独立归因。

过滤的 `m2_local_e2e_full_chain_fill` 硬编码仓库外 `D:\projects\zcodeworkspace\rust-trader\target\release\exchange.exe` 并启动进程，未在本次执行。该测试不能提供 Linux 可移植性证据。位置：[m2_local_e2e.rs:51](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/tests/m2_local_e2e.rs:51)。

## 缺陷复现与证明范围

源码：[probes.rs](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/probes.rs)。复现脚本：[run-probes.ps1](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/run-probes.ps1)。脚本链接本次构建的 release rlib，链接文件及 SHA256 见 [probe-linked-dependencies.json](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/probe-linked-dependencies.json)。

| 探针 | 关联发现 | 实际证明的内容 |
|---|---|---|
| malformed_numeric_conversion_panics | F01 | RPC 所使用的 Quantity 转换对非数字输入 panic。探针使用 unwind 捕获；生产 release 的 abort 影响由 Cargo 配置和调用路径推导，没有启动服务发送畸形请求来测量进程退出 |
| grpc_allows_cross_strategy_read_stream_and_cancel | F02、F03 | 进程内调用真实 gRPC service 方法及核心线程；Bob 可读 Alice 的快照、历史事件，并将撤单意图写入日志。没有真实执行后端，不声称验证了交易场所实际撤单 |
| registry_json_roundtrip_loses_authentication_index | F04 | JSON roundtrip 后有效 token 无法通过 authenticate |
| every_new_journal_reuses_epoch_and_first_order_id | F06 | 两个独立日志文件 epoch 都为 1，首个派生订单 ID 相同。没有实际运行双活交易节点 |
| invalid_quantity_is_persisted_then_retry_is_admitted | F08 | 零数量初次构造失败后日志已有受理；同键重试返回 already_admitted，而 Cache 没有该订单 |
| empty_scan_then_high_water_can_skip_committed_event | F09 | 按生产相同的两次读操作，在空扫描与高水位读取之间提交事件，后续从新游标扫描永远越过它。为确定性交错复现，不是依赖概率的网络压力测试 |
| expired_submit_before_is_not_enforced | F11 | clock=1,000,000 时，deadline=1 的请求仍成功受理 |
| coalesced_tcp_frames_are_merged_and_second_message_is_lost | F12 | 一次缓冲区中的 New+Fill 两帧被整体消费，只有第一帧 New 被解析 |
| partial_fill_is_translated_to_order_update | F13 | 4 股部分成交被转换为 Updated，而非 Filled |

为访问本地 FIX 私有函数，脚本从当前源码生成 `wire-source.rs` / `client-source.rs`，只将模块文档注释 `//!` 转成 `//`，再 include 原函数与追加探针；不改生产文件。生成副本与当前源码的对应关系另在完整性检查中验证。

Redis 复现：[run-redis-probe.ps1](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/run-redis-probe.ps1) 提取当前生产 `RedisConnectionConfig` trait 与 `get_redis_url` 原函数，使用最小配置对象执行 [redis-probe-main.rs](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/redis-probe-main.rs)。密码仅使用构造样例，没有读取实际凭据，也没有连接 Redis。确认 F18 的函数行为，未运行完整 Redis 集成测试。

探针独立 rustc 编译的早期尝试遇到旧 rlib 混用及 release bitcode 链接配置不匹配；等待本次 Cargo 构建完成并使用一致静态 CRT/LTO 参数后成功。这些属于复现工具准备问题，没有列为项目缺陷。最终构建仍有因 include 大段源码产生的 unused/dead_code 警告。

其余 F05、F07、F10、F14–F17、F19–F20 为当前源码与调用链审查结论；未用真实账户、磁盘故障注入、Linux ELF 或 Postgres 服务运行来扩大其验证范围。

## 未完成的生产验证

- 没有 Linux 主机编译、systemd 启停、ELF 加固检查、断电/磁盘满/旧备份恢复、容量及故障注入验收。
- 没有真实券商/交易所的 Paper/认证环境、账户和订单生命周期验收，也没有生产凭据检查。
- 没有运行全 Rust workspace 测试、make pre-commit / pre-flight、Python 测试或完整 CVE/OSV/secret 扫描。
- `python/.venv` 缺失，默认 Python 3.11 不符合项目 Python 3.12–3.14 要求；没有另建根目录虚拟环境。
- PATH 未发现 make、prek、cargo-audit、cargo-deny、cargo-vet、osv-scanner、gitleaks；没有据此宣称项目缺少对应配置或依赖无漏洞。
- stable rustfmt 忽略 `format_code_in_doc_comments`、`imports_granularity`、`group_imports`、`error_on_unformatted`；exit 0 不等于这些 nightly 格式能力经过验证。

## 工作区状态

审查开始时已有 `Cargo.lock` 3 行未提交新增。本次不变，SHA256 为 `E6EC2568CE5D38A45D6431555362B4EFCB37D9DDEC9295718C62EA84FB979BC9`。结束时 Git tracked diff 仍仅包含这 3 行；新增审查材料位于 `audit/production-review-2026-09-21/`，编译缓存位于仓库 `target/`。没有提交、推送、修改交易源码或操作远程 issue/PR。
