# P2 架构、数据和业务契约验收

日期：2026-09-30。范围：最新版
[主方案](../../../orderhub-project-simplification.md) P2 的**设计交付**。
本次没有修改 Rust 运行时代码、proto、数据库服务或外部模拟器；P4–P6
的行为验收仍未运行。先前工作区改动均予保留，无提交或推送。

| P2 条目 | 本次决定和证据 | 仍待实现的阶段 |
| --- | --- | --- |
| 本地存储 | [存储契约](../../../docs/orderhub/storage-and-commit.md)固定 redb 专属库、`schema_version=1` JSON envelope、索引/预占/投影/检查点表、同库原子事务、成功/明确失败/结果未知三分支。关闭的 DB 后端无 outbox 积压；启用前才验证真实双存储投影。 | P5 落地事务/版本；P6 检查点、备份和恢复；P8 核最终无 C/无服务路径。 |
| 首期业务范围和阈值 | [输入契约](../../../docs/orderhub/market-and-account-input.md)固定本地 `LIMIT + DAY`、USD 整股、常规时段、已确认持仓卖出和 AAPL 0.01 tick fixture；[v2 阈值](../../orderhub-simplification-2026-09-29/run-05/thresholds-v2.md)用于新鲜度、队列与小型正确性数据集。 | P4 行情/账户输入；P5 受理前能力/精度门禁。其它订单类型和实盘合约规则待新增能力验收。 |
| 模拟器接口和阻塞 | 只读核对 `D:\projects\zcodeworkspace\rust-trader` 提交 `5940c0e9e2fdc962b5b731814491c58694d1e1cd`（包 0.1.1）：FIX4.2 D/F/8 支持 DAY；REST 盘口/统计输出 `f64`、缺源时间/账户/订单/成交查询。OrderHub 本地 client 的 `query_account/query_order` 明确返回不支持。 | P4 使用精确、带时间/序号的 Rust 测试输入；P6 真实 FIX 本地闭环，查询覆盖不足时 HALTED。自动恢复需外部接口或其它可信事实来源，未声称已具备。 |
| 契约统一 | 更新受理、额度、状态、恢复四份契约，去掉旧方案和完整 LiveNode/event_store 的强制实现假设；旧方案导航已指向当前文档。 | P4–P6 逐条实施，不以文档通过代表代码通过。 |
| 模块与证据映射 | [契约验收映射](../../../docs/orderhub/contract-verification.md)列出每条合同的现有模块、正常/故障测试、预期结果及待交证据；现有 M0/M1/M2 测试只标为局部起点。 | 实施后填入真实测试文件、命令、修订哈希和运行结果。 |

## 重要的当前实现差距

- `orderhub-node/src/main.rs` 扫描日志后直接设 READY，使用不推进的
  `VirtualClock`，并初始化无源版本的硬编码账户/合约；它不满足 P2 新鲜度和
  对账门禁。`readiness.rs` 尚无完整 RECONCILING/DRAINING 状态。
- `grpc.rs` 仍接受本地模拟通道不支持的 TIF，且用可 panic 的
  `Quantity::from(String)`/`Price::from(String)` 解析外部输入；proto 缺
  请求 epoch、GetState 查询 oneof/分页。
- `journal.rs` 的成交结果只存数量，不含稳定成交 ID、价格、费用、账户及
  归属；`exec_wiring.rs` 吞掉持久化错误；当前执行事件 channel 无界。
- `local_client.rs` 的订单/账户查询不支持，外部模拟器源码也未提供所需查询；
  因此任何重启后有未知发送订单的自动 READY 都不被本次 P2 合同允许。

这些都是已登记的 P4/P5/P6 工作，不能用现有 M1/M2 测试结果或 P2 文档
将其勾成实现通过。旧 `orderhub-oms-transformation.md` 保留为历史设计来源，
与当前方案冲突的范围/阶段由主方案和本次契约覆盖。

## 证据与检查

外部源码仅做本地只读检查：`src/rest.rs` 的 DTO 用 `f64` 且 Book 仅含
`sequence`，`src/market_data.rs` 的 `published_at` 未进入 REST DTO；
`src/core/instrument.rs` 只有数字 ID/符号；`src/fix/codec.rs` 拒绝非 DAY。
没有连接外部服务，未把源代码观察说成服务运行验证。
同时清理了[美股接入选型](../../../docs/orderhub/venues-and-capacity.md)中两处
失效的本地路径：本仓库未找到所指 IB 适配器实现，文档已改为后续候选。

文档/源码哈希和静态链接检查结果见同目录 `source-hashes.txt`、
`document-checks.txt`：12 份文档、87 个本地链接检查通过，
`git diff --check` 通过。本次没有运行 Rust 构建/测试：只改了设计文档；
运行时代码将在 P4–P6 对应变更时按 `CONTRIBUTING.md` 验证。
