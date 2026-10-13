# OrderHub 文档

当前功能、启动方式和恢复流程见 [项目 README](../README.md)。

## 业务与运行

- [CentOS Stream 9 发布包与 orderhub 用户部署](orderhub/centos9-deployment.md)
- [业务运行时](orderhub/business-runtime.md)
- [准入与幂等](orderhub/admission-and-idempotency.md)
- [风控与资金、持仓预留](orderhub/risk-and-reservations.md)
- [输入运行时](orderhub/input-runtime.md)
- [行情与账户输入](orderhub/market-and-account-input.md)
- [状态与事件流](orderhub/state-and-event-stream.md)
- [本地持久化与提交](orderhub/storage-and-commit.md)
- [性能优先持久化策略](orderhub/persistence-policy.md)
- [恢复运行时](orderhub/recovery-runtime.md)
- [恢复与对账](orderhub/recovery-and-reconciliation.md)
- [本地交易场所](orderhub/local-exchange.md)
- [场所与容量](orderhub/venues-and-capacity.md)
- [TLS](orderhub/tls.md)
- [原生工具](orderhub/tooling.md)
- [执行内核拒绝码参考](orderhub/order-denied-reasons.md)

## 开发

- [贡献与验证要求](../CONTRIBUTING.md)
- [开发指南](developer_guide/index.md)
- [设计原则](developer_guide/design_principles.md)
- [文档规范](developer_guide/docs.md)
- [测试数据](developer_guide/test_datasets.md)
- [本地性能测量](../BENCHMARKING.md)

## 历史设计与验收

设计方案和历史审计保留各自的范围、时间与验收证据。
已删除的旧框架说明可通过 Git 历史查阅；历史文档不代表当前功能。

- [OMS 设计方案](../orderhub-oms-transformation.md)
- [精简方案](../orderhub-project-simplification.md)
- [报价策略变更验收](orderhub/quote-policy-v2-acceptance.md)
- [P8 验收记录](../audit/orderhub-simplification-2026-10-10/run-18/P8-acceptance.md)
