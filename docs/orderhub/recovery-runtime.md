# P6 本地恢复与运维

本文件说明 OrderHub 本期恢复实现。业务范围仍为 AAPL.GOX、GOX-001、USD、整股、
LIMIT + DAY 和显式可信的本地输入。验收见
[P6 记录](../../audit/orderhub-simplification-2026-10-02/run-16/P6-acceptance.md)。

## 恢复材料和版本

业务 envelope schema 保持版本 1；恢复格式为版本 2，登记在 redb 元数据 key 5。
完整 P5 业务库首次打开时建立封存基线，保留数量、金额、费用、归属、请求摘要和原订单 ID。
无版本旧原型库、未知版本、损坏基线及缺失历史不会作为空库或完整恢复材料接受。
旧成交如果缺少原生事件，经济账本仍可核查；Cache 恢复明确失败，须离线补充有来源的材料，
不会推测券商订单 ID、费用或成交事件。

每次业务事务将完整表变化与业务事实同 redb Immediate 提交。
连续序号、前摘要和当前摘要组成 BLAKE3 恢复链；重放后逐表与持久投影精确比较。
校验摘要用于发现损坏，不提供来自第三方的交易事实认证。
本期保留封存基线和全部增量，不实施日志回收或自动删除备份。

Cache 恢复按每页 256 条历史记录重建合约、账户、报价、订单、券商映射和原生成交持仓，
订单总数上限为 8192。精确成交 ID、数量、价格、费用、币种及归属须与经济事实一致。
恢复只更新 Cache，不广播历史命令或事件。历史输入不恢复实时流的有效性或序号；
新单仍需当前合约、合法匹配报价及账户/状态的完整快照，历史 Cache 报价不能单独开放门禁。
2026-10-11 的 `local-usd-v2` 不限制合法报价的年龄；账户/状态时效仍受检查。
初始外部持仓保留在已确认归属账本中，不伪造成交。
价格带字段仅属于内存上下文，删除它不升级业务 schema 1 或恢复格式 2。
旧订单规则版本及原 `PRICE_BAND` 拒绝保留，同键重试不会改写历史结果。

## 启动与停止

同一数据库由 redb 独占锁保护；第二个进程未取得锁不能连接执行通道。
启动先验证完整恢复链，再恢复 Cache，进入 RECONCILING。
只有没有历史业务订单且没有备份恢复栅栏的库，才允许在输入及连接均有效后持久记录 READY。
业务库无法打开或无法完整验证时启动失败，尚未连接执行通道。
业务库可读但 Cache 材料不足、连接失败或历史订单需要外部核对时保持 HALTED，保留诊断查询。

当前模拟器没有完整的账户、订单、成交及持仓查询，因此历史业务订单重启后不会自动 READY。
未发送订单保留待发状态；越过 DISPATCH_STARTED 的未终态订单视为发送未知，保留占用。
同代同键重试可返回原结果，不另造 ID，也不广播原命令。完整自动对账启用前须补齐查询覆盖。

Ctrl+C 将请求放入预留控制队列，关闭新增风险并持久记录 DRAINING。
核心线程最多用 250 ms 收取回报和排空已有工作，再记录 STOPPED 并发布一致检查点。
停止后事件订阅结束；未解决的执行占用不会因退出释放。检查点失败返回错误并尝试记录 HALTED，
节点以非零状态退出。
该时间是本地排空窗口，不是外部成交对账或部署 RTO 承诺。

## 检查点与备份

检查点从一致的 redb 逻辑读视图制作，覆盖全部业务表、索引、投影、原生恢复事件和签名密钥。
先用唯一文件名写入并同步不可变 blob，再同步 pending 清单并重命名发布 manifest。
清单包含格式、epoch、水位、blob 字节数、BLAKE3、实例 ID、二进制与配置 BLAKE3。
Unix 额外同步目录；本轮验证 Windows 进程故障语义，未认证设备断电或所有文件系统。
blob 上限 512 MiB，清单上限 16 KiB；逻辑容量预算另计恢复增量，不等于 redb 物理文件大小。

STOPPED 切面上的检查点没有未提交尾部；离线备份命令要求活动节点已释放数据库锁。
检查点 N 可与同代完整增量尾重放并核对。检查点损坏时，只能显式回退到仍通过完整校验的
封存基线及全部历史；不存在“跳过坏记录后开放新单”的路径。

备份恢复要求目标文件不存在。先在同目录 staging 库完整落盘、换代、校验，再用同文件系统
hard link 发布目标，防止覆盖并发出现的文件。文件系统须支持该操作；失败会明确报错。
恢复生成新 epoch、轮换订阅签名密钥并持久记录 HALTED。原客户订单 ID、归属和占用保留；
使用当前 epoch 可查询原订单，旧代变更请求及旧订阅 token 失效。
跨主机接管还需隔离旧实例及执行连接，本地文件锁不能证明这一步已完成。

## 运维入口

节点通过 ORDERHUB_CHECKPOINT_DIR 指定退出检查点目录，默认是 journal 路径更换扩展名为
checkpoints。离线工具使用稳定 Rust 构建，不需要 PostgreSQL、Redis 或 Python：

```bash
cargo build --locked --profile nextest -p orderhub-node --bin orderhub-recovery
orderhub-recovery inspect JOURNAL
orderhub-recovery upgrade JOURNAL
orderhub-recovery backup JOURNAL DIRECTORY
orderhub-recovery restore MANIFEST NEW-JOURNAL
```

inspect、upgrade、backup 要求已有源文件；upgrade 只封存完整 P5 材料，不补造旧经济字段。
backup 输出 manifest 路径；restore 输出换代及栅栏状态。每个入口都要求独占数据库。
节点配置摘要覆盖凭证注册表、执行地址、输入开关、监听地址、日志/检查点路径和固定 profile，
包括 `local-usd-v2` 业务规则标识；不据旧配置摘要冒充当前规则。
离线工具清单使用工具自身二进制和操作 profile 摘要，不能作为原节点配置证明。
这些摘要供核对来源，不表示自动证明新旧程序或券商规则兼容。

## 本地 FIX 验收入口

orderhub-node/tests/p6_local_fix.rs 使用显式模拟器二进制、动态端口和独立数据目录。
仅终止自己创建的子进程；配置、日志及 redb 留在运行时 TEMP 目录下的
`orderhub-p6-local-fix`。本次验证将 TEMP 指向固定工作目录的验收子目录。
测试输入为持续生成的本地 fixture，不能称作券商账户或交易所行情。

```powershell
$env:ORDERHUB_TEST_EXCHANGE_EXE = 'D:/projects/zcodeworkspace/rust-trader/target/release/exchange.exe'
cargo test --locked --offline --profile nextest -p orderhub-node --test p6_local_fix -- --ignored --test-threads=1 --nocapture
```

默认忽略需要模拟器的用例；验收必须明确启用并记录二进制哈希。
旧 m1_grpc/m2_local_e2e 夹具依赖固定端口及已退役业务接口，已迁移到真实 node 的上述入口。
原文件在本次 P5-source-baseline.zip 中可恢复。协议、原生风控、权限和经济故障用例继续保留。

本期只核销小型确定数据集、实际子进程重启及本地模拟闭环。
15 分钟备份 RPO、30 分钟自动 READY RTO、大数据量及目标部署认证仍在启用承诺前验证；
真实 PostgreSQL/Redis 服务功能验收仍在启用前独立跟踪。
P8 的 Windows x64 TLS/无 C 结果见
[P8 记录](../../audit/orderhub-simplification-2026-10-10/run-18/P8-acceptance.md)，
其他平台验证仍按延期范围跟踪；本次业务规则另行验收。
