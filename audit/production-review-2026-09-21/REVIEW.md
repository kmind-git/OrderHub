# 内网 Linux 实盘生产审查

审查日期：2026-09-21 至 2026-09-22。仓库：D:/projects/zcodeworkspace/nautilus_trader。基线：develop，HEAD `79705a0553af78c7135636061fb14390f54ca869`。

项目阶段：**用户本地开发环境中的订单管理系统**。内网 Linux 实盘是目标验收场景，本报告没有认定该系统已部署生产。代码行为问题与上线准备项分开解释；开发用模拟配置、loopback 服务或外部模拟交易所测试本身不代表已发生安全事故。

## 结论

**不建议将当前 OrderHub 应用接入真实资金生产运行。** 本次发现 20 项有明确代码依据的问题：17 项 P1、3 项 P2。P1 表示在相应功能启用或所述条件出现时必须先解决的生产阻断项；不是宣称所有问题都会在默认配置同时触发，也没有把问题泛化到 NautilusTrader 的全部上游适配器。

当前可执行入口仍硬编码 AAPL、模拟资金、静态报价、VirtualClock 和 SandboxExecutionClient；它没有通过配置装配真实执行通道。这是独立的部署能力缺口。把 gRPC 地址改成内网地址不能使这个原型成为实盘节点。证据：[crates/orderhub-node/src/main.rs:131](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:131)、[crates/orderhub-node/src/main.rs:181](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:181)、[crates/orderhub-node/src/main.rs:190](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:190)。

## 审查范围和证据边界

- 对 5,015 个版本控制文件建立结构清单，扫描 Rust/Python/协议、依赖锁定、构建、容器与配置中的相关风险模式。
- 完整阅读新增 OrderHub 三个 crate 的生产源码及协议，核对现有测试和六份设计契约；深查入口到鉴权、幂等、风控、执行、持久化、快照、补流、重启链路。
- 对 common/core/model/risk/execution/live/system/event_store/network/infrastructure/cryptography 相关路径进行定向阅读；对其余研究、回测、指标和各 venue 适配器做结构/模式扫描。
- **不是对所有 5,015 个文件逐行人工复核，也不是每个 venue 的实盘认证或完整渗透测试。** 未选定真实券商/交易所及账户，不能签发其生产可用结论。
- 实际工具环境为 Windows，Rust 1.98.1；没有 Linux 主机/交易账户可供部署验证。Python `python/.venv` 不存在，本机默认 Python 为 3.11，项目要求 3.12–3.14；没有运行 Python 项目测试。
- 源码保持只读；报告和定向复现仅写入本审查目录，构建产物位于仓库 target。原有 Cargo.lock 未提交修改予以保留，前后哈希单独记录。

P1 的判定围绕错误/越权交易、账本与回报丢失、无可靠恢复、共享交易服务中断；P2 为范围有限的可靠性或部署加固缺陷。每项都给出触发条件与修复验收方向。

F17（开发凭据/传输）与 F19（Linux 链接加固）归类为**上线配置风险**。其余发现归类为代码行为问题，并在涉及可选后端、多账户或本地 FIX 适配器时注明启用条件。优先级是面向实盘目标的修复次序，不是对当前开发环境的事故评级。

## 问题索引

| 编号 | 级别 | 问题 | 主要代码位置 |
|---|---|---|---|
| F01 | P1 | 外部请求使用会 panic 的类型转换，release 进程可被终止 | [crates/orderhub-gateway/src/grpc.rs:344](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:344) |
| F02 | P1 | 撤单只鉴权，不校验目标订单归属 | [crates/orderhub-gateway/src/grpc.rs:380](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:380) |
| F03 | P1 | 快照和事件订阅泄露其他提交方数据 | [crates/orderhub-gateway/src/grpc.rs:406](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:406) |
| F04 | P1 | JSON 凭据文件反序列化后所有 token 均无法认证 | [crates/orderhub-gateway/src/auth.rs:42](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/auth.rs:42) |
| F05 | P1 | 重启仅统计日志条数，未恢复订单和额度便宣告 READY | [crates/orderhub-node/src/main.rs:96](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:96) |
| F06 | P1 | 日志代固定为 1，旧备份或新路径可复用订单 ID | [crates/orderhub-gateway/src/journal.rs:249](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/journal.rs:249) |
| F07 | P1 | 回报落盘失败被吞掉，健康状态仍可显示可交易 | [crates/orderhub-gateway/src/sandbox.rs:177](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/sandbox.rs:177) |
| F08 | P1 | 订单构造失败后，同键重试会返回虚假的已受理 | [crates/orderhub-gateway/src/bridge.rs:285](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:285) |
| F09 | P1 | 事件流空扫描与高水位读取之间存在永久漏事件窗口 | [crates/orderhub-gateway/src/grpc.rs:447](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:447) |
| F10 | P1 | 预占账本没有接入成交、拒单和撤单结转 | [crates/orderhub-gateway/src/quota.rs:168](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/quota.rs:168) |
| F11 | P1 | submit_before 只存储不执行，过期新单仍会派发 | [crates/orderhub-gateway/src/bridge.rs:225](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:225) |
| F12 | P1 | FIX 粘包按最后一个 checksum 截断，会吞掉后续回报 | [crates/orderhub-gateway/src/local_exchange.rs:429](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/local_exchange.rs:429) |
| F13 | P1 | 部分成交被翻译成 OrderUpdated，成交量与持仓丢失 | [crates/orderhub-gateway/src/local_client.rs:132](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/local_client.rs:132) |
| F14 | P1 | 本地执行适配器把订单约束静默丢弃 | [crates/orderhub-gateway/src/local_client.rs:327](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/local_client.rs:327) |
| F15 | P1 | 对外事件缺少重建终态与成交账本所需的信息 | [crates/orderhub-proto/proto/orderhub.proto:100](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-proto/proto/orderhub.proto:100) |
| F16 | P1 | 核心入口和补流扫描没有资源上限，过载会挤压撤单 | [crates/orderhub-gateway/src/grpc.rs:100](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:100) |
| F17 | P1 | 缺少正式凭据时退回公开默认 token，内网接入仍使用明文 | [crates/orderhub-node/src/main.rs:65](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:65) |
| F18 | P2 | Redis 脱敏函数泄露短密码，并可能因 Unicode 密码 panic | [crates/infrastructure/src/redis/mod.rs:110](D:/projects/zcodeworkspace/nautilus_trader/crates/infrastructure/src/redis/mod.rs:110) |
| F19 | P2 | Linux 构建显式关闭 RELRO | [.cargo/config.toml:17](D:/projects/zcodeworkspace/nautilus_trader/.cargo/config.toml:17) |
| F20 | P2 | Postgres 缓存的可达能力仍用 todo! 终止进程 | [crates/infrastructure/src/sql/cache.rs:939](D:/projects/zcodeworkspace/nautilus_trader/crates/infrastructure/src/sql/cache.rs:939) |

## 代码发现

### F01 [P1] 外部请求使用会 panic 的类型转换，release 进程可被终止

位置：[crates/orderhub-gateway/src/grpc.rs:344](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:344)（第 344–348 行）。关联：[crates/model/src/types/quantity.rs:930](D:/projects/zcodeworkspace/nautilus_trader/crates/model/src/types/quantity.rs:930)、[Cargo.toml:496](D:/projects/zcodeworkspace/nautilus_trader/Cargo.toml:496)。

通过鉴权的提交方只需提交 quantity="not-a-number"、无效 instrument_id 等输入。RPC 直接调用 Quantity::from、Price::from 和标识类型的 From 包装；Quantity 的 From<String> 在解析失败时 expect。根 Cargo.toml 的 release profile 使用 panic="abort"，因此这不只是单次 INVALID_ARGUMENT，而可能终止整个交易进程，影响其他策略及撤单。

修复与验收：统一使用可失败的 from_str/new_checked，在进入核心线程前校验字段长度、非空 request_id、数量、价格和标识；错误映射为 INVALID_ARGUMENT。用 release 子进程发送畸形 RPC，断言服务仍存活。

### F02 [P1] 撤单只鉴权，不校验目标订单归属

位置：[crates/orderhub-gateway/src/grpc.rs:380](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:380)（第 380–389 行）。关联：[crates/orderhub-gateway/src/bridge.rs:496](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:496)。

CancelOrder 获得 credentials 后只传 submitter_id 与目标订单 ID；bridge 从全局 Cache 查到订单后直接使用该订单的 strategy_id 派发撤单。Bob 的 token 即使只授权 DESK-B，也可以撤销 DESK-A 的已知订单。订单 ID 可由查询泄露或其顺序结构获知，不能作为权限凭据。

修复与验收：在服务端查询目标订单的持久归属，验证凭证的策略集合和撤单权限，再持久化撤单意图。未知订单、无权订单必须采用不泄露归属的响应。加入跨策略和重启后撤单负向测试。

### F03 [P1] 快照和事件订阅泄露其他提交方数据

位置：[crates/orderhub-gateway/src/grpc.rs:406](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:406)（第 406–429 行）。关联：[crates/orderhub-gateway/src/grpc.rs:479](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:479)。

GetState 与 WatchEvents 仅验证 token 是否存在，随后分别返回全局 active_orders、遍历整个业务日志，未按 credentials.strategies 或 submitter_id 过滤。任一有效 token 都能获取其他提交方的策略、订单、请求和成交状态。

修复与验收：把授权视图带入核心快照和日志扫描；为 Outcome 保存可核验归属；将恢复游标绑定授权身份与权限版本。账户全局视图使用独立权限。测试两个互不授权的提交方，覆盖快照、历史补流和实时事件。

### F04 [P1] JSON 凭据文件反序列化后所有 token 均无法认证

位置：[crates/orderhub-gateway/src/auth.rs:42](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/auth.rs:42)（第 42–46 行）。关联：[crates/orderhub-node/src/main.rs:59](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:59)。

SubmitterRegistry 的 by_token 标记为 serde(skip)，Deserialize 不会调用 new()。ORDERHUB_SUBMITTERS 启动路径直接 serde_json::from_str<SubmitterRegistry>，得到有 submitters 但空 by_token 的对象，authenticate 恒为 None。new() 的重复 token 检查也不会执行。这会使正式多提交方配置启动后所有 RPC 被拒绝，而硬编码 fallback 路径看似正常。

修复与验收：反序列化原始配置后调用经过校验的构造函数，或实现受校验的 Deserialize；校验空 token、重复 token、身份与策略。增加 JSON roundtrip 和通过配置文件启动服务的测试。

### F05 [P1] 重启仅统计日志条数，未恢复订单和额度便宣告 READY

位置：[crates/orderhub-node/src/main.rs:96](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:96)（第 96–110 行）。关联：[crates/orderhub-node/src/main.rs:128](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:128)、[crates/orderhub-node/src/main.rs:200](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:200)。

启动时 scan_all()?.len() 仅计算 recovered_entries，紧接着就设置 READY；core_setup 随后建立全新的 Cache、模拟账户和空 QuotaLedger。旧订单、持仓、预占和未完成撤单没有重建，也没有执行通道对账。旧请求可由日志返回 already_admitted，但快照中没有订单；新的请求则按重置后的额度受理。READY 还早于核心线程 setup 完成。

修复与验收：恢复订单、策略持仓、预占与业务投影，证明未知派发状态得到保守处理；完成连接和账户对账后才打开 gate。core setup 使用显式成功握手，任何失败保持 HALTED。用进程 kill/restart 验证有挂单、部分成交和待撤时的完整状态。

### F06 [P1] 日志代固定为 1，旧备份或新路径可复用订单 ID

位置：[crates/orderhub-gateway/src/journal.rs:249](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/journal.rs:249)（第 249–254 行）。关联：[crates/orderhub-gateway/src/bridge.rs:563](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:563)、[crates/orderhub-proto/proto/orderhub.proto:20](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-proto/proto/orderhub.proto:20)。

缺失 META_EPOCH 时永远取 1，没有换代流程；新日志 seq 从 0 开始。derive_client_order_id 使用 epoch 与 seq，所以删除/改错日志路径或回退备份后可能重新发出既有订单 ID。SubmitOrder、CancelOrder 与 WatchEvents 协议也没有请求日志代字段，无法拒绝旧客户端命令或旧代游标。redb 对同一文件的锁不能防止使用不同数据目录的两个节点。

修复与验收：持久化唯一日志身份与受控恢复代号，在协议中验证期望 epoch，恢复旧备份必须完成换代和对账；加入独立的数据目录/部署实例校验及单活隔离。不能用重启时无条件换代代替正常幂等连续性。

### F07 [P1] 回报落盘失败被吞掉，健康状态仍可显示可交易

位置：[crates/orderhub-gateway/src/sandbox.rs:177](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/sandbox.rs:177)（第 177–182 行）。关联：[crates/orderhub-gateway/src/worker.rs:212](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/worker.rs:212)、[crates/orderhub-gateway/src/grpc.rs:416](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:416)。

attach_outcome_recorder 对 worker.submit_outcome 的错误使用 let _ 丢弃。worker 的写入错误只发送给等待者，没有驱动 ReadinessGate。引擎 Cache 已应用的成交或撤单可能未持久化，GetState 仍按 gate 返回 writable=true，WatchEvents 只能看到缺失回报的日志。bridge 的写失败也未把 gate 置为 HALTED。

修复与验收：把持久化失败、提交结果不确定和 worker 退出连接到统一 fail-closed 门禁；保守保留额度，标记查询 stale，禁止继续以正常高水位发布状态。增加 ENOSPC、I/O 错误与写线程故障注入测试。

### F08 [P1] 订单构造失败后，同键重试会返回虚假的已受理

位置：[crates/orderhub-gateway/src/bridge.rs:285](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:285)（第 285–295 行）。关联：[crates/orderhub-gateway/src/bridge.rs:322](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:322)、[crates/orderhub-gateway/src/bridge.rs:526](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:526)。

Admission 在 LimitOrder::new_checked 和 Cache.add_order 之前持久化。quantity=0 等可解析但不能构造订单的输入首次返回错误，日志却留下幂等锚点；receipt_for_existing 不要求订单创建成功，也不区分无 outcome 或拒绝 outcome，直接返回 already_admitted。gRPC 将其转换为 ADMITTED，形成不存在订单的成功回执。

修复与验收：在持久受理前完成可预先验证的构造校验；对提交之后的失败写入明确终态并返回稳定的原结果；恢复中的不确定状态独立表达。测试非法首次请求、同键重试以及受理落盘后崩溃。

### F09 [P1] 事件流空扫描与高水位读取之间存在永久漏事件窗口

位置：[crates/orderhub-gateway/src/grpc.rs:447](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:447)（第 447–453 行）。关联：[crates/orderhub-gateway/src/journal.rs:402](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/journal.rs:402)。

scan_after(cursor) 与 last_seq() 使用不同读事务。若扫描得到空结果后发生一次提交，last_seq 读到新序号并直接推进 cursor，下一次扫描只读更大的序号，刚提交事件永远不会交付，客户端还收到表示已应用该位置的 progress 帧。这违反快照加补流无缺口的契约。

修复与验收：游标只能推进到实际扫描/交付过的位置；让扫描结果与扫描高水位来自同一快照，或空扫描时保持 cursor 并重新扫描。用屏障强制在两次读取之间提交，验证每条业务事件至少交付一次。

### F10 [P1] 预占账本没有接入成交、拒单和撤单结转

位置：[crates/orderhub-gateway/src/quota.rs:168](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/quota.rs:168)（第 168–171 行）。关联：[crates/orderhub-gateway/src/quota.rs:316](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/quota.rs:316)、[crates/orderhub-gateway/src/bridge.rs:375](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:375)、[crates/orderhub-node/src/main.rs:200](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:200)。

QuotaLedger 只有入口预占被 bridge 调用。生产组装中没有调用 add_position 或 release_committed，也没有按订单保存与释放卖出预占的生命周期。结果是拒单/撤单后的买入额度持续占用，买入成交也不增加策略可卖数量；main 设置的单一策略预算迟早耗尽，而所有卖出现有仓位的请求仍被当作零持仓处理。账户总预算配置也未在 main 调用。

修复与验收：基于持久成交 ID、订单 ID 建立幂等账本，成交结转持仓/资金/费用，拒单与撤单终态仅释放未成交部分；同时配置账户总限额。全链路测试买入后卖出、部分成交再撤单、重复回报与恢复。

### F11 [P1] submit_before 只存储不执行，过期新单仍会派发

位置：[crates/orderhub-gateway/src/bridge.rs:225](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:225)（第 225–239 行）。关联：[crates/orderhub-gateway/src/bridge.rs:282](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:282)、[crates/orderhub-node/src/main.rs:181](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:181)。

submit_before_ns 只参与指纹与日志序列化，在准入和派发前都没有比较当前时间。即使业务截止时间已过仍受理；排队后更无法避免迟发。main 使用不推进的 VirtualClock，日志时间和原生限频时间也不具备真实运行语义。

修复与验收：生产使用实时钟；首次准入与首次派发前均检查业务截止时间，已经受理但未派发的过期订单写入终态并释放预占。同键重试首先返回原结果；不要用 RPC 连接的存活状态判断业务是否过期。

### F12 [P1] FIX 粘包按最后一个 checksum 截断，会吞掉后续回报

位置：[crates/orderhub-gateway/src/local_exchange.rs:429](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/local_exchange.rs:429)（第 429–438 行）。关联：[crates/orderhub-gateway/src/local_exchange.rs:410](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/local_exchange.rs:410)。

find_frame_end 使用 rposition 查找最后一个 10=。当一次 TCP read 收到连续两帧时，会把两帧一同从 buffer drain；parse_fields/field 只用第一组字段解码一个事件，第二个成交或撤单回报被无声丢弃。它还没有依据 BodyLength 验证帧边界和校验和。影响当前本地 FIX 模拟适配器，不意味着上游所有 venue 适配器具有同一缺陷。

修复与验收：按 BeginString、BodyLength 和严格的 checksum 字段解析一帧，保留余下字节继续解码；限制最大帧长度并处理错误。测试任意分片、2 到 N 帧粘包、字段值中的 10= 和损坏长度/校验和。

### F13 [P1] 部分成交被翻译成 OrderUpdated，成交量与持仓丢失

位置：[crates/orderhub-gateway/src/local_client.rs:132](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/local_client.rs:132)（第 132–158 行）。关联：[crates/orderhub-gateway/src/local_exchange.rs:290](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/local_exchange.rs:290)。

ExecEvent::Fill 在 complete=false 时生成 OrderUpdated，沿用原订单 quantity/price，完全不产生经济成交。10 股分为 4+6 成交时，先到的 4 股不会计入成交和持仓，最后一笔只以 last_qty=6 生成 Filled；cum_qty 被忽略。TradeId 又固定为 T-{client_order_id}，不能正确标识一个订单的多次成交。

修复与验收：每笔部分或最终成交都生成 OrderFilled，使用场所稳定 ExecID/成交 ID 去重，保留 last_qty、last_px、累计量和费用。以部分成交、分多笔完结、重传、撤单竞态验证订单与持仓一致。

### F14 [P1] 本地执行适配器把订单约束静默丢弃

位置：[crates/orderhub-gateway/src/local_client.rs:327](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/local_client.rs:327)（第 327–339 行）。关联：[crates/orderhub-gateway/src/local_exchange.rs:552](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/local_exchange.rs:552)、[crates/orderhub-gateway/src/grpc.rs:331](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:331)。

LocalExchangeClient 从订单取 side、quantity、price 后就发送，没有校验 time_in_force、post_only 或 reduce_only；FixWriter 总发送 DAY。gRPC 却只接受 GTC/IOC/FOK、不接受 DAY。接入该适配器后，IOC/FOK 会变成可以继续留簿的 DAY，post_only/reduce_only 也没有兑现或拒绝。当前 main 用 Sandbox，问题在启用 LocalExchangeClient 时触发。

修复与验收：在受理前和适配器边界执行能力矩阵，支持的语义准确映射，不支持的组合明确拒绝；协议加入真实支持的 DAY。逐项断言 wire TIF 与 unsupported flags 的错误结果。

### F15 [P1] 对外事件缺少重建终态与成交账本所需的信息

位置：[crates/orderhub-proto/proto/orderhub.proto:100](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-proto/proto/orderhub.proto:100)（第 100–105 行）。关联：[crates/orderhub-gateway/src/grpc.rs:491](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:491)、[crates/orderhub-gateway/src/grpc.rs:212](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:212)、[crates/orderhub-gateway/src/sandbox.rs:153](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/sandbox.rs:153)。

OutcomeEvent 仅有 origin_seq、client_order_id、outcome 和 reason；即使 JournalOutcome::Filled 保存 last_qty，gRPC 转换也将数量丢弃，更没有成交价、成交 ID、费用和持仓。GetState 只返回非终态订单，协议没有 OrderLookup/RequestLookup。客户端错过终态后无法靠状态快照恢复完整结果，也无法按事件正确累计部分成交。recorder 只处理 Denied/Filled/Canceled，场所 Rejected、Accepted、Expired 等未形成完整业务投影。

修复与验收：先定义可重建的持久业务事件与按请求/订单查询接口，再用同一已提交版本生成快照与事件流；覆盖必要订单终态、成交、账户和持仓。事件去重使用稳定 ID，不能仅向客户端发送一个 FILLED 标签。

### F16 [P1] 核心入口和补流扫描没有资源上限，过载会挤压撤单

位置：[crates/orderhub-gateway/src/grpc.rs:100](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:100)（第 100–106 行）。关联：[crates/orderhub-gateway/src/journal.rs:402](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/journal.rs:402)、[crates/orderhub-gateway/src/bridge.rs:396](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/bridge.rs:396)、[crates/orderhub-gateway/src/worker.rs:169](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/worker.rs:169)。

核心入口使用无界 std::sync::mpsc::channel；Submit、Cancel、Snapshot 共用 FIFO。bridge 同步等待每次持久化，入口没有条数/字节/等待时间限制和撤单预留。另一个放大点是 scan_after 将全部尾部日志物化为 Vec，GetState 在核心线程扫描全部历史记录，每个订阅独立读盘。输出 channel(256) 只约束发送端缓冲，不能约束前面已物化的历史数据。

修复与验收：入口按请求和字节设全局/提交方许可，受理前明确 RESOURCE_EXHAUSTED；为撤单与对账保留调度份额。补流分批并限制订阅数量/积压，投影增量维护。用长历史加多订阅、慢消费者及新单洪峰测试内存和撤单 P99。

### F17 [P1] 缺少正式凭据时退回公开默认 token，内网接入仍使用明文

分类：上线配置风险。当前本地开发默认绑定 loopback 的行为不单独作为漏洞；以下风险针对以后跨主机部署时仍沿用该配置的情况。

位置：[crates/orderhub-node/src/main.rs:65](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-node/src/main.rs:65)（第 65–73 行）。关联：[crates/orderhub-gateway/src/grpc.rs:548](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/src/grpc.rs:548)、[crates/orderhub-proto/Cargo.toml:30](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-proto/Cargo.toml:30)。

未配置 ORDERHUB_SUBMITTERS/ORDERHUB_TOKEN 时服务自动使用源码中的 orderhub-dev-token，并授予 trader-1 的策略权限。默认监听 127.0.0.1 限制了直接远程访问，但运维只要把 ORDERHUB_ADDR 改为内网 IP 或 0.0.0.0，就会把此默认凭据暴露给网络可达者；serve 没有 TLS 配置，token 在直接远程连接上是明文。未发现可审查的 TLS 终止代理部署配置。

修复与验收：生产模式缺少有效凭据即拒绝启动；示例 token 只能在显式开发模式使用。跨主机链路使用认证加密或经过验证的 mTLS 代理并约束监听面，配套凭据轮换。内网位置不能替代身份和授权。

### F18 [P2] Redis 脱敏函数泄露短密码，并可能因 Unicode 密码 panic

位置：[crates/infrastructure/src/redis/mod.rs:110](D:/projects/zcodeworkspace/nautilus_trader/crates/infrastructure/src/redis/mod.rs:110)（第 110–117 行）。关联：[crates/infrastructure/src/redis/mod.rs:168](D:/projects/zcodeworkspace/nautilus_trader/crates/infrastructure/src/redis/mod.rs:168)、[crates/infrastructure/src/redis/mod.rs:305](D:/projects/zcodeworkspace/nautilus_trader/crates/infrastructure/src/redis/mod.rs:305)。

redact_pw 对长度 <=4 的密码直接返回原值；长密码保留头尾两字节，并按字节切 UTF-8 字符串。配置 pass 会完整进入 debug 的 Connecting 日志；非 ASCII 密码在非法字符边界切片时可 panic。现有 full_config_with_ssl 测试甚至把包含 pass 的 redacted_url 当作期望。

修复与验收：日志中的密码一律固定为 *** 或完全省略，禁止输出前后缀；URL userinfo 独立做正确编码。增加短密码、多字节 Unicode 和保留字符用例，检查所有日志与错误格式化路径。

### F19 [P2] Linux 构建显式关闭 RELRO

分类：上线配置风险。当前主机为 Windows；以下结论针对仓库中的 Linux 发布构建选项。

位置：[.cargo/config.toml:17](D:/projects/zcodeworkspace/nautilus_trader/.cargo/config.toml:17)（第 17–17 行）。关联：[Cargo.toml:489](D:/projects/zcodeworkspace/nautilus_trader/Cargo.toml:489)。

Linux target rustflags 加入 -Wl,-z,norelro，显式关闭链接器对重定位相关区域的只读保护。这是实际构建配置，降低生产 Linux 二进制的利用缓解能力，不是已发现可利用的内存破坏漏洞。当前 Windows 验证不能证明 Linux 最终 ELF 的加固状态。

修复与验收：删除该关闭项并在发布构建明确启用合适的 RELRO/BIND_NOW，测试兼容性和性能。使用目标主机 readelf/checksec 验证 GNU_RELRO、BIND_NOW、NX 与 PIE，记录实际发布制品哈希。

### F20 [P2] Postgres 缓存的可达能力仍用 todo! 终止进程

位置：[crates/infrastructure/src/sql/cache.rs:939](D:/projects/zcodeworkspace/nautilus_trader/crates/infrastructure/src/sql/cache.rs:939)（第 939–940 行）。关联：[crates/common/src/cache/mod.rs:2481](D:/projects/zcodeworkspace/nautilus_trader/crates/common/src/cache/mod.rs:2481)、[crates/infrastructure/src/sql/cache.rs:880](D:/projects/zcodeworkspace/nautilus_trader/crates/infrastructure/src/sql/cache.rs:880)。

选择 Postgres 缓存后，Cache::add_synthetic 会调用 database.add_synthetic，此实现直接 todo!。delete_actor、delete_strategy、add_order_book 等也有 panic 占位。默认 release 的 abort 会把不支持的功能变为进程级故障。此问题只在选择该后端并调用相应能力时触发；当前 OrderHub redb 路径不受这项直接影响。

修复与验收：对所有不支持能力返回可诊断的 Result 错误，在配置/启动阶段拒绝不兼容组合；如生产需要该能力再实现并测试。不要把可选功能不支持表示为进程 panic。

## Linux 主机部署仍缺少的交付与验证

以下是上线验收项，未把“仓库未提供”直接当作服务器已配置错误。

| 领域 | 仓库当前证据 | 上线前要求 |
|---|---|---|
| 运行入口 | main 固定模拟配置，没有真实通道选择与账户加载 | 显式区分模拟/实盘模式、绑定真实通道、校验合约与账户；真实时钟和数据新鲜度门禁 |
| 服务生命周期 | gRPC 使用 serve，没有 shutdown future；核心/执行引擎以 mem::forget 维持生命周期 | SIGTERM 关闭新单入口、排空已受理与回报、确认 durable commit，再退出；验证强杀恢复 |
| systemd | 未发现 OrderHub service/unit 或运维 runbook | 专用非 root 账户，绝对数据路径、严格目录权限、启动校验、有限重启、受控停止时限 |
| 运行隔离 | 未提供生产 Linux 隔离配置 | 按实际 systemd 版本验证 NoNewPrivileges、ProtectSystem、只写数据目录、能力边界与资源限制；这些是部署建议，非已验证配置 |
| 健康与监控 | Readiness 未接入持久化/执行故障；没有该入口的指标端点 | 监控真实可交易状态、队列深度/年龄、存储延迟/剩余空间、执行连接、最后回报/行情时间、对账差异 |
| FIX 会话 | 本地实现无完整心跳/会话序号恢复；reader 退出仅 eprintln，connected 标志不随之清除 | 仅作为模拟适配器；进入生产路径前补齐会话和订单恢复、断线门禁、读写超时及回报去重 |
| 备份/单活 | redb 本地事务；没有完整 checkpoint + epoch/fencing 流程 | 一致备份、恢复演练、旧节点隔离、磁盘故障 RPO/RTO、校验与回退 runbook |
| 离线/内网构建 | Cargo/uv 锁文件、工程工具版本及供应链政策存在 | 内部镜像/缓存与完整依赖供应、锁定构建、SBOM、漏洞扫描结果和制品签名/哈希入库 |
| 容量 | 现有性能用例是直接多线程调用 PersistenceWorker 写日志 | 通过真实入口开启鉴权、预占、执行回报和事件扇出压测；分别报 admitted/dispatch/venue ack、拒绝率与 P50/P95/P99 |
| 发布/回滚 | 没有 OrderHub 数据 schema 版本和迁移/回滚方案 | 二进制、协议、配置、数据兼容矩阵；先验证数据兼容再允许回滚 |

不能将现有 `m1_c1_mixed_submit_and_cancel_durable_throughput` 认作 1000 提交/s + 1000 撤单/s 的生产证据：该用例并发写 Admission 与 Canceled outcome，没有通过 gRPC/核心串行链路，没有真实撤单确认，也没有额度与对账。参考 [crates/orderhub-gateway/tests/m1_acceptance.rs:284](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/tests/m1_acceptance.rs:284)。没有在本报告宣称完成吞吐目标。

systemd 隔离选项的含义参照 [systemd 官方执行环境文档源码](https://github.com/systemd/systemd/blob/main/man/systemd.exec.xml)；必须按目标 Linux/systemd 版本验收。内网身份边界参照 [NIST SP 800-207](https://csrc.nist.gov/pubs/sp/800/207/final)，不能仅因网络位于局域网而给予隐式信任。gRPC 的流控作用于流式 RPC，不能替代本应用的无界核心队列限制，参照 [gRPC 官方流控说明](https://grpc.io/docs/guides/flow-control/)。

## 已有能力与排除的误报

- 上游 LiveNode 已有启动对账、启动失败处理、SIGINT/SIGTERM 与关闭流程；不能因为自定义入口没用这些能力而说上游全部缺失。
- OrderHub redb 提交明确使用 Immediate durability、checked_add 分配序号；问题在跨步骤状态机和恢复接线，不能说完全未持久化。
- 请求 submitter_id 在 SubmitOrder 中由服务端凭证覆盖，strategy_id 提交路径有授权检查；漏洞集中在撤单、读取和加载配置后的索引。
- 网络库 TLS 路径使用证书根与 ServerName 验证，本次模式扫描未发现关闭证书验证的生产调用证据；这不是完整 TLS 安全认证。
- .docker/docker-compose.yml 的数据库被文档明确定位为本地测试，端口绑定 loopback。默认测试密码没有直接按“公网暴露数据库”报告。
- 金额核心使用 Price/Quantity/Decimal；本次没有用“项目存在 float”笼统判为资金精度漏洞。
- SQL 缓存 load_actor/load_strategy 已返回明确不支持错误，文档也披露边界；F20 专指仍会 panic 的可达方法。

## 验证状态

最终运行结果见 [VALIDATION.md](D:/projects/zcodeworkspace/nautilus_trader/audit/production-review-2026-09-21/VALIDATION.md) 与原始日志。报告中的代码路径结论与运行复现分别标记；测试通过不表示已完成实盘上线验收。

单独重跑的 OrderHub 现有测试为 **26 passed、0 failed、1 filtered out**。审查专用程序为 **13 passed**（9 个缺陷探针及随原代码包含的 4 个 FIX 单测），另有 2 个 Redis 函数断言成立。缺陷探针“通过”表示复现当前错误行为。缺陷与探针的对应关系、对照条件及证明边界详见验证记录。

日志吞吐用例初次运行约 1,425 writes/s，低于 2,000 的断言门槛；待同时进行的探针编译结束后，单独重跑约 7,278 writes/s 并通过。保留两次日志，没有修改测试门槛，也不把短时本机结果外推为实盘容量。

全工作区 `cargo fmt --all -- --check` 返回 0，但当前 stable rustfmt 对四项 nightly-only 配置给出忽略警告，不能把它等同于所有规定格式能力均被检查。

现有 `m2_local_e2e_full_chain_fill` 无条件使用仓库外硬编码 Windows 路径并启动模拟交易所，因此没有执行。这既不是 Linux 可移植验证，也不能在未指定外部交易所环境时算作本次已覆盖。证据：[crates/orderhub-gateway/tests/m2_local_e2e.rs:51](D:/projects/zcodeworkspace/nautilus_trader/crates/orderhub-gateway/tests/m2_local_e2e.rs:51)。

未运行 make pre-commit / pre-flight、全部 Rust workspace / Python 测试、CVE/OSV 全扫描、Linux 编译与故障演练。当前 PATH 未发现 make、prek、cargo-audit、cargo-deny、cargo-vet、osv-scanner、gitleaks。供应链检查配置存在不等于检查结果通过，本报告不声称依赖无漏洞。

## 建议修复顺序与退出条件

1. 封住请求和权限边界：F01–F04、F17；畸形输入不崩溃，两个身份不能互查互撤，正式 JSON 配置可用。
2. 修复 durable admission/恢复/额度：F05–F08、F10–F11；杀进程、写盘失败、旧备份、部分成交再撤单后账本正确，未对账不 READY。
3. 修复事件与适配器：F09、F12–F15；粘包分片无丢失，逐笔成交准确，终态可查询，断线重连后完整补流。
4. 处理过载和部署：F16、F18–F20 与部署表；目标 Linux 上验证真实发布制品、systemd 生命周期、监控、备份恢复和端到端容量。
5. 选择具体生产通道，以其 Paper/认证环境完成账户、订单、回报、费用、日历、断线恢复测试。每一阶段都重跑相应负向测试，不能仅凭“正常下单成功”放行。

本次没有修改交易实现、提交代码、推送或操作任何远程 issue/PR，也未连接真实交易账户。
