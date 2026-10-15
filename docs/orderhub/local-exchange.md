# 本地模拟交易所接入（rust-trader）

状态：P6 真实 node/FIX 本地闭环已接入，最终结果见 [P6 记录](../../audit/orderhub-simplification-2026-10-02/run-16/P6-acceptance.md)。
修订日期：2026-10-15。返回 [项目精简方案](../../orderhub-project-simplification.md)；
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

## FIX session 配置文件

节点通过 `ORDERHUB_FIX_CONFIG` 指定文件，未设置时读取当前工作目录下的
`config/orderhub-fix.cfg`。源码默认模板见[配置文件](../../config/orderhub-fix.cfg)。
发布包中的模板为 `deploy/orderhub-fix.cfg`，复制到实例自己的配置目录后修改。

```ini
[DEFAULT]
ConnectionType=initiator
BeginString=FIX.4.2
HeartBtInt=30
Logging=Y
FileLogPath=../logs/fix
FileLogMaxBytes=10485760
FileLogBackups=5

[SESSION]
SenderCompID=ORDERHUB
TargetCompID=GOX
SocketConnectHost=127.0.0.1
SocketConnectPort=5001
```

只支持一个 `[SESSION]`，可以覆盖 `[DEFAULT]` 中的同名设置。
这是 OrderHub 支持的配置子集；不是完整 QuickFIX 配置解析器。
未知键、重复键、第二个 session、缺少必填值或不支持的 FIX 版本都会在打开业务库前报错。
`ConnectionType` 必须为 `initiator`，`BeginString` 必须为 `FIX.4.2`。
CompID 为 1–64 个 ASCII 字母、数字、点、下划线或连字符；双方 ID 必须不同。
端口范围 1–65535，心跳间隔范围 1–3600 秒。IPv6 host 写 `::1`，不加方括号。
仅支持整行 `#`/`;` 注释，配置文件最大 64 KiB，启动时读取，修改后重启生效。

模拟交易所使用另一个 acceptor 配置文件，其 `SenderCompID`/`TargetCompID` 必须与这里互换，
`HeartBtInt` 相同，`SocketAcceptPort` 对应这里的 `SocketConnectPort`。
会话身份可配置，当前 OMS 本地业务输入仍限定 AAPL.GOX / GOX-001 / ORDERHUB-S001。

旧 `ORDERHUB_EXCHANGE_ADDR` 已退出节点配置；仍设置该变量会明确提示迁移。
迁移时移除旧变量，把 host/port 写入上述文件，并设置 `ORDERHUB_FIX_CONFIG`。
缺少文件不会回退到默认地址。恢复身份哈希包含解析后的有效 session 和日志设置。

## FIX message 和 event 日志

`FileLogPath` 相对配置文件所在目录解析，接受 `/` 或 `\` 分隔符。
默认模板会生成以下文件，时间戳为 UTC RFC 3339 毫秒：

- `FIX.4.2-ORDERHUB-GOX.messages.current.log`：`in` 为完整接收帧，`out` 为成功发送帧。
  包含登录、心跳、TestRequest、合约定义、订单、撤单及执行回报；保留原始 SOH 分隔符。
- `FIX.4.2-ORDERHUB-GOX.event.current.log`：连接尝试/失败、登录、心跳、探测应答、
  Logout、传输/解析错误、业务回报队列溢出和节点退出。
- `FIX.4.2-ORDERHUB-GOX.log.lock`：防止不同进程同时写入并轮转同一组文件。
  不同实例须使用独立日志目录。空锁文件可保留，进程退出后 OS 自动释放锁。

配置的 CompID 会进入文件名。单个文件默认 10 MiB，轮转后保存 `.1` 到 `.5`，
`.1` 为最近的历史文件；message/event 各自最多约 60 MiB。
`FileLogMaxBytes` 支持 64 KiB–1 GiB，`FileLogBackups` 支持 1–20。
`Logging=N` 显式关闭这两类日志，业务 journal 的策略不受该设置影响。

日志采用独立线程和 4096 条有界队列，发送线程不等待文件写入，也不为日志执行 fsync。
队列满时丢弃诊断记录，stderr 提示 `FIX_LOG_QUEUE_FULL`，随后在 event 中记录
`LOG_RECORDS_DROPPED count=...`。运行中写盘失败提示 `FIX_LOG_ERROR` 并停止诊断记录，
订单处理继续；启动时无法创建文件或取得日志锁则直接失败。
因此这些文件是尽力记录的诊断日志，业务持久化和恢复依据仍是 redb journal。
正常停止时尝试在 2 秒内刷新已排队记录到 OS 文件缓冲区。

Linux 查看报文分隔符：

```bash
tr '\001' '|' < ~/orderhub/logs/fix/FIX.4.2-ORDERHUB-GOX.messages.current.log | tail -n 20
tail -n 20 ~/orderhub/logs/fix/FIX.4.2-ORDERHUB-GOX.event.current.log
```

## FIX 会话保活

登录后由 FIX 会话线程保活，不依赖下单、撤单或核心线程主动拉取回报：

- 距上次成功发送达到配置的 `HeartBtInt` 时发送 `Heartbeat (35=0)`，订单等成功发送也重置该计时。
- 收到 `TestRequest (35=1)` 时立即回复 `Heartbeat (35=0)`，原样回带 `TestReqID (112)`。
- 心跳与订单、撤单共用一个发送锁和递增的 `MsgSeqNum (34)`；锁只保护 FIX 发送状态。
- 心跳等会话消息不进入业务回报队列。业务回报队列满、网络错误、缺少或空 TestReqID
  或收到 Logout 时关闭传输，核心线程检测回报通道断开后进入 `HALTED`。

会话轮询最长等待 250 毫秒，登录响应等待和单次写入超时为 5 秒。
这些保活能力不提供 FIX 序号持久化、ResendRequest 重传或自动业务恢复对账。
回归测试包含本地 TCP 对端应答/并发序号测试，以及显式指定模拟器的 120 秒空闲后下单撤单测试。

## 不支持项的处置原则

按方案能力表原则：客户端请求了该通道不支持的指令组合时明确拒绝并
回带稳定原因码，不静默丢弃字段、不伪造支持。
