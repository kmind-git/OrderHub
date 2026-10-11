# P6 验收记录

日期：2026-10-02。状态：P6 本期本地 profile 验收通过；A09–A12 按主方案的启用边界核销。
本期可运行隔离模拟 MVP。自动外部查询对账、部署 RPO/RTO 和 P8 TLS/无 C 矩阵继续单独跟踪。
实现与命令见 [恢复运行时](../../../docs/orderhub/recovery-runtime.md)，路线见
[精简方案](../../../orderhub-project-simplification.md)。

## 修订与证据

- 仓库：本地 OrderHub 派生仓库，原 Git HEAD 为 479d8674d80af212044731ef43c9a24db30aa40f，当前 HEAD 未变。
- stable Rust 1.98.1，Windows MSVC；使用 nextest profile，未重建 target/debug。
- 原始材料和哈希见 [baseline.json](baseline.json)、[实施前哈希](implementation-start-hashes.json)和
  [P5 源码归档](P5-source-baseline.zip)。保留其他阶段已有本地修改，未提交或推送。
- 最终 [54 项源码/配置哈希](tested-source-hashes.json)与
  [14 项实际命令及耗时](validation.json)对应；收尾记录见
  [最终状态核对](final-state-validation.json)和[文档哈希](document-hashes.json)。
- 运行脚本为 [verify-p6.ps1](verify-p6.ps1)，文档及哈希复核为
  [verify-p6-docs.ps1](verify-p6-docs.ps1)。开发失败日志、首次/第二次门禁归档继续保留，未覆盖为成功。
  首次门禁中 Rust 检查均通过，但 README 更新触发全文件哈希检查；最终源码清单排除 Markdown，文档另行核对。

## 已实现内容

| 本期目标           | 实现与验证                                                                                                                   |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| A09 完整本地恢复   | 格式 2 封存基线与原子恢复增量；校验连续序号、BLAKE3 链及完整表投影；从已提交事实恢复订单、券商映射和原生成交持仓。           |
| A10 不确定状态门禁 | RESTORING/RECONCILING、HALTED、DRAINING/STOPPED；历史输入不变为新鲜输入；原 ID、请求结果及不确定占用保留；不广播历史命令。   |
| A11 备份与单实例   | 一致逻辑 blob 和同步 manifest；完整 P5 材料升级、旧备份换 epoch/签名密钥、无覆盖恢复；同进程及真实第二进程均拒绝活动数据库。 |
| A12 真实模拟闭环   | 真实 node、HTTP/2 四 RPC、真实 FIX 模拟器；部分/全部成交、撤单、订阅、断线、重启及原请求重试；缺外部查询时持续 HALTED。      |

原生事件恢复另外核对账户、策略、合约、订单、方向、类型、成交 ID、数量、价格、费用及币种。
缺少原生材料的旧成交保留经济记录并明确拒绝 Cache 恢复，不猜测缺失信息。
两个旧 FIX 夹具的消费者已迁移，精确文件、原哈希及替代入口见
[退役清单](retired-fixtures.json)；原文件保存在本次源码归档。

## 最终检查结果

14 项检查全部返回 0；**155 个测试通过、0 失败**。
1 个 ignored 为真实杀进程用例调用的子进程辅助入口，其四个阶段由父用例实际执行。
另 1 个 filtered 为已有日志吞吐 benchmark，本期不重复运行它。

| 检查                                                 | 通过测试数       | 日志                                                              |
| ---------------------------------------------------- | ---------------- | ----------------------------------------------------------------- |
| fmt、四个 OrderHub crate 的 all-targets check/Clippy | 编译与 lint 门禁 | [格式](format.log)、[check](check.log)、[Clippy](clippy.log)      |
| lib/bin 单元测试                                     | 25               | [单元日志](unit-tests.log)                                        |
| P6 恢复/备份/停机/真实进程边界                       | 16               | [恢复日志](p6-recovery.log)                                       |
| P6 离线工具参数与备份恢复                            | 1                | [运维日志](p6-operator.log)                                       |
| P6 真实 node/FIX                                     | 2                | [模拟闭环日志](p6-local-fix.log)                                  |
| P5 经济事务/额度                                     | 18               | [业务回归](p5-business.log)                                       |
| P5 查询与订阅                                        | 8                | [API 回归](p5-api.log)                                            |
| P4 输入与原生链                                      | 74               | [输入回归](p4-inputs.log)                                         |
| 原生/权限回归                                        | 5 + 6            | [原生](native-regression.log)、[权限](auth-regression.log)        |
| dependency-features / test-network                   | 两项策略通过     | [依赖策略](dependency-policy.log)、[网络策略](network-policy.log) |

完整命令与编译/链接耗时以 validation.json 为准；上述测试执行时间不含 Cargo 编译。
文档表格、Markdown lint、本地文件链接、diff 空白和最终哈希核对以 final-state-validation.json 为准。
哈希对应本次实际源文件；不将这些日志套用到之后修改的修订。

## 故障与数据量

| 场景                 | 数据与结果                                                                                                                                                                                             |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 四次真实辅助进程终止 | 受理前、受理后、DISPATCH_STARTED 后、成交提交后；水位分别 0/1/2/3。未受理、待发、发送未知、已成交分别恢复；重试不增加订单或序号。                                                                      |
| 原生恢复             | 1 个订单，2 股、150.00 USD、0.50 USD 费用，SUBMITTED/ACCEPTED/FILL；Native Cache 数量/映射、账本及无命令广播一致，历史输入仍不就绪。                                                                   |
| 检查点/升级/损坏     | 0–1 个订单的小型固定数据；N 检查点加尾部核对、经济字段往返相同；基线/投影/增量/版本/未发布清单错误明确失败；写失败保留此前完整备份。                                                                   |
| 停机                 | 成功时持久 STOPPED 并验证检查点；失败时返回错误并持久 HALTED；两条 HTTP/2 路径的事件订阅都结束。node 将信号/停机错误传播为非零退出。                                                                   |
| FIX 闭环             | 买 10 股，卖 4 股，买单成交 4 股后撤剩余 6 股；订阅看到两份已提交成交，原请求重试返回原订单。断线与重启保持 HALTED；重启时水位 30、2 个业务订单。                                                      |
| RPC ACK 后杀 node    | 买 3 股，已确认 ADMITTED；真实第二进程先因 DatabaseAlreadyOpen 拒绝启动；立即杀主进程后，原订单 O-1-000000000019 及 460.66 USD 占用保留。重启水位 20、1 个发送未知订单，新请求不可用，同键取回原结果。 |

16 个恢复测试总执行时间 **2.64 秒**；离线工具测试 **1.88 秒**；
两个真实 node/FIX 场景共 **4.90 秒**。这些为本机小数据功能结果，不作容量、p99 或自动 READY RTO 认证。
磁盘写失败沿用 P5 的真实 redb 同步失败/事务回滚回归；P6 补充确定性的检查点路径故障及损坏材料，未反复填满磁盘。
辅助进程终止核销持久边界，真实 node 用例补充 RPC 已确认和实际 FIX 发送/回报后的进程恢复；
未宣称在每个机器指令时间点都进行外部网络杀进程。

## 模拟器及保留现场

显式运行 D:/projects/zcodeworkspace/rust-trader/target/release/exchange.exe；没有修改或重建外部仓库。
原源码版本引用不视作此二进制的构建来源证明。

- SHA-256：CAA0DBD3FAB069EFF400D32FF107CE45B09C52B68ED3275A847A379E948AF290。
- BLAKE3：5435a93a89bfe1ec888c1d1254698f5a4b6aac466f83fe7ffcb9b1278ddd34ba。
- 闭环 FIX/gRPC 端口：55512/55514；ACK 崩溃场景 FIX/gRPC：55539/55541。
  REST 端口和会话参数保存在各现场配置；测试客户端只访问 loopback，不声明模拟器所有监听均仅绑定 loopback。
- 现场一：[node.log](runtime-tmp/.tmpvTLnE0/node.log)、[exchange.log](runtime-tmp/.tmpvTLnE0/exchange.log)、[FIX 配置](runtime-tmp/.tmpvTLnE0/fix.cfg)。
- 现场二：[node.log](runtime-tmp/.tmpJu7dFi/node.log)、[第二实例拒绝日志](runtime-tmp/.tmpJu7dFi/second-owner.log)、[FIX 配置](runtime-tmp/.tmpJu7dFi/fix.cfg)。

只终止测试自己创建并持有句柄的子进程；没有操作其他已运行实例。
行情、账户与初始归属为持续生成的本地 fixture，不作为券商对账证据。

## 后续启用边界

1. 模拟器缺完整订单/成交/持仓查询，历史业务库及备份恢复后持续 HALTED。
   人工证据核查不等于已具备自动解闸接口；补齐查询覆盖并验收后才能启用自动对账/READY。
1. 完整 P5 材料可封存升级。无版本旧原型、未知 schema 或缺原生事件须离线补证，不能补造经济字段。
1. 本期不回收完整恢复链/防重索引；硬链接发布需文件系统支持。跨主机接管须先隔离旧实例。
1. 未认证操作系统 Ctrl+C/SIGTERM 投递、所有文件系统断电语义、Linux/macOS 运行矩阵或大数据量恢复。
   15 分钟备份 RPO、30 分钟自动 READY RTO 在相应部署/容量承诺前完成。
1. 真实 PostgreSQL/Redis 服务功能继续按启用前清单延期；保留路径无 C 要求不变。
   当前 gRPC 为本地明文 profile；最终 TLS/无 C 构建链接及启用矩阵在 P8 核销。

下一阶段为 P7 框架收尾，P7/P8 尚未验收。
