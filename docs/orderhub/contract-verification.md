# P2 契约到代码与验收映射

状态：P4 已通过本地 fixture 验收；P5/P6 已通过各自本地 profile 验收。
P8 Windows x64 本期结果见
[P8 记录](../../audit/orderhub-simplification-2026-10-10/run-18/P8-acceptance.md)。
其他行的“现有测试”只表示可复用起点，**不表示新契约已通过**。
项目路线见[精简方案](../../orderhub-project-simplification.md)，本期阈值见
[v2](../../audit/orderhub-simplification-2026-09-29/run-05/thresholds-v2.md)及
[P5 补充](../../audit/orderhub-simplification-2026-10-01/run-15/P5-thresholds.md)。
每次 P4–P6/P8 实施应在对应测试文件和运行记录中填入真实命令、修订哈希、
夹具/服务版本、结果与失败；计划文件尚不存在时不得写成已验证。

2026-10-11 的 `local-usd-v2` 覆盖下表原输入用例中的报价时效/缺口阻断预期：
报价没有年龄上限，完整合法报价可跨缺口，单独报价断流沿用最后合法值；
旧帧/非法帧不回退估值。账户/状态时效、元数据、现金/归属和恢复门禁继续有效。
额度用例增加越过原 ±10% 价格带仍可受理、历史 `PRICE_BAND` 拒绝同键重试不改写。
API/落盘后发送及真实本地 FIX 增加陈旧报价仍可写/发送验证。
当前规则以[输入契约](market-and-account-input.md)和[主方案修订](../../orderhub-project-simplification.md)为准，
原 P4/P5/P8 记录不能直接证明本次规则通过；补充命令/哈希在主方案登记的固定工作目录留痕。

| 契约与阶段                                      | 当前 Rust 落点；后续职责                                                                                                                                                   | 正常测试与预期                                                                                                                                                 | 故障测试与预期                                                                                                                                            | 应交证据                                                                                                                                                                    |
| ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [输入](market-and-account-input.md)，P4         | `orderhub-gateway/src/inputs.rs`、`bridge.rs`、`grpc.rs` 和 `orderhub-node/src/main.rs`：有界输入、持续推进的实时钟及核心线程定时回调，连接 `Cache`/`Portfolio`/准入门禁。 | `p4_inputs.rs`：精确 AAPL 输入，整数股/tick 的 `LIMIT + DAY` 进入原生风控链；当前规则增加旧报价、合法缺口和单独报价断流仍可写。                                | 报价旧帧/非法帧保留原合法值；账户/状态过期、覆盖缺失、停牌、元数据冲突和整体输入断开仍阻断；授权撤单保持独立。                                            | [P4 验收](../../audit/orderhub-simplification-2026-10-01/run-14/P4-acceptance.md)的 73 个 A01 用例属于原规则；本次补充验收单独留痕。                                        |
| [受理/幂等](admission-and-idempotency.md)，P5   | `orderhub-gateway/src/grpc.rs`、`auth.rs`、`bridge.rs`、`journal.rs`、`worker.rs`；`orderhub-proto/proto/orderhub.proto` 增加 epoch/能力字段和查询形态。                   | `p5_business.rs`、`p5_api.rs`：同键同参数只一订单/预占/发送意图，持久提交后才 ACK；不支持能力持久拒绝。                                                        | 同文件：同键不同参数冲突、真实 redb 同步失败、未提交事务回滚及队列超时；提交/回复/发送前后真实杀进程在 P6。                                               | [P5 运行记录](../../audit/orderhub-simplification-2026-10-01/run-15/P5-acceptance.md)记录本地组件证据；P6 进程故障独立核销。                                                |
| [额度](risk-and-reservations.md)，P5            | `orderhub-gateway/src/ledger.rs`、`journal.rs`、`quota.rs`、`bridge.rs`、`exec_wiring.rs`，以及 `crates/risk/src/engine`。                                                 | `p5_business.rs`：两笔争用预算仅一笔受理；买入预算、卖出可用持仓、账户总约束用精确数值同事务结转。                                                             | 同文件及 `p4_inputs.rs`：重复/乱序成交、撤单竞争、写入未知、迟到费用、成本舍入、极值/步进错误不重复释放或越权放行。                                       | [P5 运行记录](../../audit/orderhub-simplification-2026-10-01/run-15/P5-acceptance.md)及其测试日志。                                                                         |
| [存储/提交](storage-and-commit.md)，P5/P6       | `orderhub-gateway/src/journal.rs`、`worker.rs`、`exec_wiring.rs`；版本 envelope、完整事件/投影、错误传播与格式 2 检查点/恢复增量已实现。                                   | `p5_business.rs`：请求/额度/发送意图/事件/投影同 redb 提交，读回同一水位；版本 1 解码及旧原型库识别并拒绝打开；完整 P5 材料升级及缺材料拒绝见 p6_recovery.rs。 | 同文件及 journal 单元测试：同步失败未知、不发布失败事实、保留占用并 HALTED；校验和/代号/逻辑容量错误拒绝。损坏检查点、基线及增量尾拒绝见 p6_recovery.rs。 | [P5 运行记录](../../audit/orderhub-simplification-2026-10-01/run-15/P5-acceptance.md)及 [P6 记录](../../audit/orderhub-simplification-2026-10-02/run-16/P6-acceptance.md)。 |
| [状态/补流](state-and-event-stream.md)，P5      | `orderhub-gateway/src/grpc.rs`、`journal.rs` 与 `orderhub-proto/proto/orderhub.proto`；GetState oneof、授权快照和持久游标。                                                | `p5_api.rs`：分页固定 N，WatchEvents 从 N 连续持久扫描；每个事件读取当次不可变投影，原请求/终态可查询。                                                        | 同文件及 grpc 单元测试：篡改/跨身份 token、慢消费者、满租约、64 新单及 128 查询洪峰；撤单仍有独立容量。                                                   | [P5 运行记录](../../audit/orderhub-simplification-2026-10-01/run-15/P5-acceptance.md)及其 API/单元日志。                                                                    |
| [恢复/对账](recovery-and-reconciliation.md)，P6 | `orderhub-node/src/main.rs`、`orderhub-gateway/src/readiness.rs`、`journal.rs`、`local_client.rs`；OrderHub 自有生命周期与检查点。                                         | `p6_recovery.rs`：同代完整日志重建请求、订单、归属、额度、投影；真实 FIX 本地闭环见 `orderhub-node/tests/p6_local_fix.rs`。                                    | 同文件：旧备份、发送未知、缺成交/订单查询、进程崩溃、重复回报均不盲发，保持 HALTED 或对应占用。                                                           | [P6 记录](../../audit/orderhub-simplification-2026-10-02/run-16/P6-acceptance.md)：真实进程终止、原生 Cache、备份换代与停机。                                               |
| [本地通道](local-exchange.md)，P5/P6            | `orderhub-gateway/src/local_exchange.rs`、`local_client.rs`；FIX4.2 DAY/限价能力表和缺查询门禁。                                                                           | `p6_local_fix.rs`：对外部源码版本固定的模拟器运行提交、撤单、部分/全部成交与重连，核对稳定成交 ID。                                                            | 同文件：GTC/IOC/FOK、market、post/reduce 在发出 FIX 前拒绝；断线期间无覆盖查询时不进入 READY。                                                            | [P6 记录](../../audit/orderhub-simplification-2026-10-02/run-16/P6-acceptance.md)：真实 node/FIX HTTP/2 闭环；查询不足仍 HALTED。                                           |
| DB 保留无 C，P3/P8/启用前                       | `crates/infrastructure` Redis/PostgreSQL；SQLx provider 原型与本地补丁；保留 schema/API。                                                                                  | P8 生产 R/P/PT/U 选定 feature 构建、链接及本地 TLS/配置通过；DB 启用前再测真实认证、事务/消息、重连。                                                          | TLS 错名/过期/不受信、provider 冲突不降级；未启用 DB 不产生 outbox 积压或 READY 故障。                                                                    | [P1 技术证据](../../audit/orderhub-simplification-2026-09-30/run-08/P1-acceptance.md)及上述 P8 记录；真实服务功能仍在启用前验收。                                           |

P5 代码已加入 epoch/查询 oneof、持久原结果、同事务额度和事件投影、稳定成交 ID、
完整经济字段、分页/签名 token、有界回报和持久化错误门禁。
具体实现和阈值见 [P5 运行时](business-runtime.md)；最终验收记录见
[P5 证据](../../audit/orderhub-simplification-2026-10-01/run-15/P5-acceptance.md)。
P6 接入检查点、完整材料升级、Cache 重建、RECONCILING/DRAINING、真实进程恢复及模拟器集成；
实现边界见 [恢复运行时](recovery-runtime.md)，本期验收见 [P6 记录](../../audit/orderhub-simplification-2026-10-02/run-16/P6-acceptance.md)。
缺原生事件或外部查询证据的库保持 HALTED，不补造旧经济字段。
P8 原范围内的 TLS/无 C 结果由上述记录核销；真实 DB 启用前验证和其他延期项继续独立跟踪。
