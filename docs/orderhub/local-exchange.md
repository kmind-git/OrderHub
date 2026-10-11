# 本地模拟交易所接入（rust-trader）

状态：P6 真实 node/FIX 本地闭环已接入，最终结果见 [P6 记录](../../audit/orderhub-simplification-2026-10-02/run-16/P6-acceptance.md)。
修订日期：2026-10-02。返回 [项目精简方案](../../orderhub-project-simplification.md)；
行情、账户和恢复查询缺口见[输入契约](market-and-account-input.md)。

## 目标系统

`D:\projects\zcodeworkspace\rust-trader`（gotrader）：价格-时间优先撮合、手写
FIX 4.2 会话层（acceptor :5001）、只读 REST（:8080）、回放行情（playback）。
P2 核对其本地源码提交 `5940c0e9e2fdc962b5b731814491c58694d1e1cd`、包版本 `0.1.1`。
P6 显式运行本机现有 release/exchange.exe；源码提交不视为该二进制的构建来源证明。
运行时固定二进制 SHA-256/BLAKE3、独立配置、动态端口和本次日志。入口与边界见
[P6 运行时](recovery-runtime.md)。没有修改外部仓库或测试其他已运行实例。

## 接入参数

- FIX acceptor：`127.0.0.1:5001`（`SocketAcceptPort` 可配）
- 会话：CompID 由 acceptor 的 `[SESSION]` 声明准入（未声明即拒）；
  OrderHub initiator 需要独立的 SenderCompID（建议 `ORDERHUB`）并在
  acceptor 配置中声明，或使用 `DynamicSessions=Y`
- REST（只读，无鉴权）：`/api/instruments/`、`/api/book/{SYMBOL}`、
  `/api/stats/{SYMBOL}`、`/api/sessions`

## 能力矩阵（测试范围依据）

| 能力                       | 支持                                          | 测试策略                                                                                   |
| -------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------ |
| 限价单（OrdType=2）        | ✅                                            | 首期 `LIMIT + DAY` 全流程测试                                                              |
| 市价单（OrdType=1）        | ✅，但本期 OMS 不受理                         | 后续有价格保护方案和风控验收再启用                                                         |
| 有效期 DAY（TIF=0）        | ✅（仅此一种）                                | 全流程测试                                                                                 |
| 撤单（35=F）               | ✅                                            | 撤单竞争测试                                                                               |
| 改单（35=G，撤单替换）     | ✅                                            | 暂不接入（OrderHub 无 ModifyOrder）                                                        |
| 拒单回报（ExecType=8）     | ✅                                            | 非法参数拒单测试                                                                           |
| 部分成交（ExecType=1）     | ✅                                            | 限价单挂簿后逐步成交测试                                                                   |
| ExecType New/2/4/5/C       | ✅                                            | 随全流程断言                                                                               |
| TIF GTC/IOC/FOK            | ❌                                            | 在 OrderHub 受理前拒绝，不发送到 FIX                                                       |
| post_only / reduce_only    | ❌ 无对应 FIX 字段                            | 不测试；能力表披露，不静默丢弃                                                             |
| 冰山/条件单/组合单         | ❌                                            | 不测试                                                                                     |
| 行情 FIX 订阅（W/推送）    | ❌（playback 单向）                           | OrderHub 不经该通道取行情                                                                  |
| 行情 REST 轮询             | ✅（只读快照，价格/数量为 JSON 浮点）         | 仅作诊断；无源时间/心跳和精确十进制，不作为准入行情                                        |
| 账户、订单、成交、持仓查询 | ❌                                            | 自动恢复对账阻塞；OrderHub `query_account/query_order` 返回不支持                          |
| 合约                       | IBM/AAPL/AMZN/GOOG/FB/NFLX/ORCL/700/9988/1810 | 以 AAPL 为主                                                                               |
| 价格/数量精度              | 撮合内部用 Decimal；REST 输出 `f64`           | FIX 值保留精度；本地 AAPL 0.01 tick/整股由 OrderHub fixture 验证，模拟器品种表没有这些字段 |

HandlInst 必须为 1（自动化私有执行）；HeartBtInt 必须为正。

## 不支持项的处置原则

按方案能力表原则：客户端请求了该通道不支持的指令组合时明确拒绝并
回带稳定原因码，不静默丢弃字段、不伪造支持。
