# local-usd-v2 业务规则补充验收

日期：2026-10-11。Windows x64 / Rust stable 1.98.1 本地范围通过。
实施方案见[价格保护与报价门禁移除方案](quote-policy-v2-plan.md)。
本次在 P0–P8 精简验收后移除两个业务功能，原阶段记录保留原规则的证据范围。

## 当前行为

- 不再设置相对卖价的 ±10% 价格带，订单保留客户端提交的精确限价。
- 报价不设年龄或源到接收延迟上限；仅报价停更、断连或完整顶层报价跳号不阻断新单。
- 估值/敞口使用最后合法且匹配当前定义的报价，保留真实时间/来源；旧帧或非法帧不覆盖它。
  启动及合约修订变化后仍需合法匹配报价。停更可能使估值偏离当前市场。
- 独立报价重连使用新输入代和完整快照建立序号基线，旧代排队帧丢弃，账户/状态不重置。
- 账户 10 秒、交易状态 5 秒、权限、tick/数量、现金/持仓、费用、单笔/敞口限额、截止时间、
  FIX、持久化及恢复门禁保留。共享 JSONL EOF 或未知类型损坏仍阻断关键流。
- 受理、落盘后发送及 READY/API 健康采用同一规则。`writable/stale` 不承诺报价实时。
- 新订单规则版本为 `local-usd-v2`，纳入节点配置摘要；业务 schema 1、恢复格式 2、RPC 不变。
  旧 `PRICE_BAND` 拒绝同键重试返回原结果，新请求 ID 才按新规则判断。

## 实际验证

全部使用 `--locked --offline --profile nextest`，关闭 debug 符号及增量缓存。
复用仓库 target；清理继承的适配器变量；运行数据保留在专用本地 TEMP/TMP。

| 目标              | 实际执行通过 | 源码入口                                              |
| ----------------- | ------------ | ----------------------------------------------------- |
| P4 输入           | 80           | crates/orderhub-gateway/tests/p4_inputs.rs            |
| P5 业务           | 22           | crates/orderhub-gateway/tests/p5_business.rs          |
| 应用 lib/bin      | 28           | gateway/node/proto/tools                              |
| API               | 9            | crates/orderhub-gateway/tests/p5_api.rs               |
| 恢复/进程故障     | 16           | crates/orderhub-gateway/tests/p6_recovery.rs          |
| 估值              | 2            | crates/portfolio/tests/portfolio.rs，两个精确过滤用例 |
| 真实本地 node/FIX | 2            | crates/orderhub-node/tests/p6_local_fix.rs            |
| 合计              | 159          | 仅计实际执行通过项                                    |

恢复入口另有一个 ignored 子进程 helper，由真实进程边界用例显式调用，不计为额外通过项。
两个估值用例为 `test_price_overrides_are_fresh_and_leave_cached_query_values_unchanged`
和 `test_closing_position_updates_portfolio`，各过滤其他 156 项，未声明完整套件通过。

格式、四个应用全部 targets 的 check/Clippy（`-D warnings`）、依赖 feature/测试网络策略、
Git diff 及 11 份文档检查均通过。完整应用命令见 [CONTRIBUTING](../../CONTRIBUTING.md)。
集成入口分别为 gateway 的 `p4_inputs`、`p5_business`、`p5_api`、`p6_recovery`；
真实 FIX 使用 node 的 `p6_local_fix -- --ignored --test-threads=1 --nocapture`。

本地 FIX 只发送首条报价，持续更新账户/状态；报价年龄超过 2 秒后，限价 170 的买单
进入实际 FIX，随后部分成交 4 股、撤单 CANCELED；FIX 断连关闭门禁，重启 HALTED，
原 ID/原结果仍可重试。第二用例验证 RPC ACK 后终止进程，保留占用 460.66 和数据库独占锁。
模拟器只读使用现有二进制，运行前后 SHA256 均为：

```text
CAA0DBD3FAB069EFF400D32FF107CE45B09C52B68ED3275A847A379E948AF290
```

旧拒绝兼容测试通过当前持久接口构造 v1 原结果，再重开库验证不重新判断；
未执行旧二进制或验证降级。首次 API 字段误用、Clippy 写法和价格规范化断言已修复并重跑，
完整失败日志保留在本地记录中。

## 证据与范围

验收源码基于 HEAD `479d8674d80af212044731ef43c9a24db30aa40f` 上的未提交工作区；
提交前已逐文件核对没有源码漂移。原完整记录位于：

```text
D:/projects/codexworkspace/orderhub-quote-policy-2026-10-11/implementation-01/
```

该目录有原方案、23 文件备份/哈希、本次 20 文件差异、12 项 Cargo 门禁日志、
测试统计、15 个执行产物哈希和 final-proof.json。本页是可在仓库中阅读的摘要，
本地编译产物、运行数据库和审计缓存不属于源码交付。

本次 Cargo.toml/Cargo.lock 和 feature 未变，原
[P8 Windows 无 C/TLS 证据](../../audit/orderhub-simplification-2026-10-10/run-18/P8-acceptance.md)
沿用原范围，未重跑完整矩阵。未连接 PostgreSQL/Redis，服务启用前验证仍保留。
跨平台、容量、部署 RPO/RTO、真实券商/资金和自动对账不属于本次验收。
