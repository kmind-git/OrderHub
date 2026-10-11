# P4 行情与账户输入验收

日期：2026-10-01。结论：**通过本地 fixture 范围的 P4/A01 验收**。
主方案见[精简方案](../../../orderhub-project-simplification.md)，实现见
[输入说明](../../../docs/orderhub/input-runtime.md)。不扩大已冻结的业务范围或阈值。

## 实施结果

- 新增 `orderhub-gateway::inputs` 的有界 Rust 接口和 JSONL 本地适配入口：1,024 条
  默认队列，单条 JSONL 最大 16 KiB，字段长度/归属条目有限，输入连续性丢失需完整重同步。
- 合约、完整顶层报价/连续替换、交易状态/日历和账户/持仓分别有源时间与序号。
  合约修订改变或相同修订冲突后，报价与状态需重新同步。
- 金额、价格和数量按十进制字符串解析，更新核心线程的 Cache/Portfolio；外部持仓
  与已确认的策略归属分开，不伪造成交或原生 Position。
- 节点不再预置合约、百万美元账户与静止的 VirtualClock；使用持续推进的 LiveClock，
  核心线程注册/消费有界实时定时事件，验证 RustLocal 回调在其核心线程执行。
- 按 2/5/10 秒阈值及 500 ms 保守源到接收时间差执行门禁；重复/旧序号不刷新新鲜度，
  缺口、未知状态、断连、停牌、过期、覆盖不足和错误修订阻断新增风险。
- 受理前与日志提交后、进入原生执行链前校验能力、当前 tick、整数股、现金购买力、
  卖出归属及截止时间。动态条件失效的已提交请求记录 Denied，不进入原生执行链。
- gRPC 数量/价格和标识符使用 fallible 解析；不支持能力返回稳定原因。
  GetState 的 writable 包含输入门禁，并能在首笔订单前返回已加载合约。
- 撤单读取指定已提交订单的归属，校验提交方及当前策略权限；不因新单输入门禁关闭。

## 数据与运行范围

固定 fixture：`crates/orderhub-gateway/test_data/p4_inputs.json`，4 类初始快照，
1 个 AAPL.GOX 合约、1 个 GOX-001 USD 现金账户、1 个授权归属策略。
价格增量 0.01、整股；外部持仓 25 股，其中 20 股确认归属 ORDERHUB-S001。
权限用例配置 2 个提交方。队列饱和实际覆盖 4 条及 1,024 条输入。

Rust 1.98.1 stable，Windows x86_64-pc-windows-msvc，nextest profile，
不要求 nightly、Python、数据库实例或外部交易所进程。
固定 fixture 时间由测试的可控时钟解释；运行节点的接收时间使用实际实时钟。
该 fixture 不是券商/模拟器账户事实，历史 fixture 不可改本地时间戳冒充新鲜外部数据。

## 验收证据

完整命令、时间和退出码见 [validation.json](validation.json)，可重跑入口为
`verify-p4.ps1`。Rust 源码、两个 manifest、Cargo.lock 与 fixture 共 11 个输入的
SHA-256 见 [tested-source-hashes.json](tested-source-hashes.json)；最终所有检查完成后
逐项验证未改变。Git HEAD 只用于定位基础提交，本仓库仍有历史阶段的本地改动，
不能只用 HEAD 代表测试修订。

收尾记录 `final-state-validation.json` 核对全部 9 个 Rust 检查及 11 个源码输入，
哈希无偏差；6 份相关 Markdown 的 lint 为 0 问题，45 个本地文件链接全部存在，
git diff --check 退出 0。没有把外部 URL 或锚点检查声称为已通过。

| 检查                                   | 实际结果                                         |
| -------------------------------------- | ------------------------------------------------ |
| stable cargo fmt --all -- --check      | 退出 0                                           |
| 四个 OrderHub 目标 --all-targets check | 退出 0                                           |
| 同目标 Clippy -D warnings              | 退出 0                                           |
| 四个 OrderHub 目标 --lib --bins        | 17 通过，0 失败/忽略                             |
| p4_inputs.rs                           | 73 通过，0 失败/忽略/过滤，测试执行 0.79 秒      |
| 原生链路/幂等/日志回归                 | 5 通过；1 个未受本次影响的写入成本基准未选择执行 |
| 权限/额度/就绪/恢复读回回归            | 6 通过，0 失败/忽略/过滤                         |
| orderhub-check dependency-features     | checks-only feature，退出 0                      |
| orderhub-check test-network            | checks-only feature，退出 0                      |

A01 覆盖正常缓存/组合账户更新、原生风控链入口、重复/乱序、缺口、完整重同步、
精确时限及 1 ns 越界、未来时间、500 ms 边界、逐消息的运行时钟检查、合约修订与
同序号冲突、未知交易状态、交易时段、账户覆盖/归属、精度/资金、断连、溢出、
JSONL 编解码与 EOF、提交期间过期/截止，以及授权撤单和 gRPC 错误路径。
实时钟推进及核心线程定时回调另有实际运行用例，不依靠长时间等待触发陈旧边界。

开发期先后修复测试请求字段遗漏、Clippy 所有权/借用建议和两个时钟/元数据边界；
历史日志保留，不把早期失败当最终通过。Windows 以外完整 metadata 的离线查询曾
因未缓存 linux-raw-sys 失败，随后按本期 Windows 目标取得 metadata；不声明 Linux 验收。
中途继续编辑触发了一次源码哈希保护，最终冻结源码后重新执行全部上述检查并通过。

## 本阶段之外

- 本阶段未连接 Redis/PostgreSQL，也未启动真实 FIX 模拟器或完整 orderhub-node 进程。
  真正订单/回报闭环与恢复归 P6；最终应用 R/P/PT/U 构建、链接和运行矩阵归 P8。
- P5 继续完成事务受理、额度/持仓预占、成交账本、接口/事件流和其队列容量治理。
  输入门禁不能代替这些一致性保证。
- P6 继续核销生命周期与外部对账；当前日志扫描不能证明可以自动恢复交易。
- 实际外部行情、日历、账户接口、可信时钟与服务启用另行验证；没有重新定义来源能力。
- 完整 prek、跨平台环境和实盘未运行；不以本阶段测试声称它们通过。

未提交、推送或发布；原有许可证、P3 备份和历史 python/.venv 保留。
