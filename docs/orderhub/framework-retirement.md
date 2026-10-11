# P7 框架退役与历史数据入口

当前范围是 15 个 Rust workspace member。P7 已按本阶段范围验收，见
[P7 验收记录](../../audit/orderhub-simplification-2026-10-02/run-17/P7-acceptance.md)；最终无 C 和跨平台矩阵属于 P8。

## 职责迁移

- 生命周期、检查点、备份和恢复：OrderHub node 与 gateway 的 P4–P6 实现。
- Cache、时钟、消息总线、OMS 数据、执行、风险及组合估值：保留核心 crate。
- 研究统计、回测、策略框架、通用 LiveNode/system/event_store 和通用 catalog：退役。
- 必要历史 Arrow 编解码：迁到 `orderhub-tools/legacy-catalog`，默认节点不启用 Arrow/Parquet。
- Redis 双总线与 Cache 恢复断言：迁到 infrastructure 独立集成测试；默认忽略实际服务执行。
- testkit：仅保留 Cache 内存替身及事件收集。纯 Rust HMAC/Ed25519 签名继续保留。
- Cap'n Proto/SBE 与 RSA/旧 TLS provider 汇聚接口：断开消费者后退役。JSON/MsgPack、应用 TLS 和证书要求保留。

原始文件、版权和源语言说明可从
[逐文件源码归档](../../audit/orderhub-simplification-2026-10-02/run-17/pre-p7-source.zip)
及 [SHA-256 清单](../../audit/orderhub-simplification-2026-10-02/run-17/pre-p7-source-hashes.json) 恢复。
历史开发文档的源码链接指向归档中的原路径；其中旧 Python、研究与回测命令不是当前运行入口。

## 离线历史迁移

```bash
cargo run --locked --profile nextest -p orderhub-tools --no-default-features --features legacy-catalog --bin orderhub-migrate -- parquet --source old.parquet --destination converted.v1.jsonl --record-type QuoteTick
cargo run --locked --profile nextest -p orderhub-tools --no-default-features --features legacy-catalog --bin orderhub-migrate -- verify --artifact converted.v1.jsonl
```

`high-precision` 可与 `legacy-catalog` 同时启用。工具处理内建行情、账户、订单/持仓事件、报告、快照和合约类型；
不支持任意 CustomData 或推测未知 schema。旧十档平铺深度布局显式转换，空档必须是零值尾部填充。

输出是版本 1 JSONL：源文件 BLAKE3、连续行序号、原生精确数值字符串、事件计数、数量/金额总计和记录摘要。
读取后回验原生类型和完整摘要；未知版本、缺失字段、错序、损坏或不完整结果拒绝。
该摘要验证内部一致性；需要证明与原文件对应时应保留原文件并核对源摘要，不能将摘要视作签名。

原文件不修改；发布前再次核对源摘要。输出路径不得存在，临时输出验读后使用不覆盖的同目录发布。
目录必须存在且文件系统支持硬链接。单源上限 128 MiB、输出 512 MiB、100 万行、单行 1 MiB；超限显式拒绝。
工具不自动导入交易账本，不恢复为 READY。

历史文件的 Zstandard 解码使用
[本地 Parquet 补丁](../../patches/parquet-59.3.0/ORDERHUB-PATCH.md)。它只提供纯 Rust 读取，Zstandard 写入显式拒绝；
帧窗口和单页解码限制 64 MiB，校验内容摘要、页大小和完整输入。其他压缩写入不属于该离线转换入口。

真实 PostgreSQL/Redis 的认证、事务和重连验证保留为启用门禁；本阶段编译服务测试并核查无 C 依赖特性。
