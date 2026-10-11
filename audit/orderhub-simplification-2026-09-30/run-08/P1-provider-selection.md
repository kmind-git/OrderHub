# P1 TLS 候选采用评估

日期：2026-09-30。范围：本仓库锁定的 `rustls 0.23.45`、
`rustls-graviola 0.4.0`、`graviola 0.4.1` 与 `rustls-webpki 0.103.15`，
交付平台仅为 `x86_64-pc-windows-msvc`、Rust stable 1.98.1。

## 技术与维护边界

| 核对项 | 证据与结论 |
| --- | --- |
| 实现来源 | [Graviola 维护方说明](https://github.com/ctz/graviola)声明使用 Rust 和内嵌汇编，无需 C 编译器；部分低层汇编来自 s2n-bignum。汇编符合当前“无第三方 C/C++ 实现”定义，须在构建输入记录中单独标明。 |
| 维护状态 | 维护方明确称项目仍很新，需谨慎采用；[发布记录](https://github.com/ctz/graviola/releases)及近期变更显示维护活动，但不能据此推断独立安全审计或长期支持承诺。 |
| 平台/CPU | 维护方仅列 x86-64/aarch64；x86-64 必须具备 AES、SSSE3、AVX、AVX2、ADX、BMI2、PCLMULQDQ。本机检测和 TLS 握手已通过；缺失位注入测试通过。其他 CPU/系统不在本期支持承诺内。 |
| TLS 算法 | 锁定源码的 provider 包含 TLS 1.2/1.3 的 AES-GCM、ChaCha20-Poly1305、RSA/ECDSA 验证与 X25519/P256/P384 等；还提供默认的混合 ML-KEM 组。本地原型已测 TLS 1.2/1.3、RSA/ECDSA 证书及成功、过期、错名和不受信证书路径。混合组和实际交易所互操作尚未验收。 |
| 已知公告 | 2026-09-30 查阅 [RustSec 包清单](https://rustsec.org/packages/)和针对两个 Graviola crate 的公开检索，未找到专属公告；**未检索到不等于证明无漏洞**。[RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285.html)对 rustls 要求 `>=0.23.45`，当前锁定版本满足。[RUSTSEC-2026-0104](https://rustsec.org/advisories/RUSTSEC-2026-0104.html)、[0098](https://rustsec.org/advisories/RUSTSEC-2026-0098.html)、[0099](https://rustsec.org/advisories/RUSTSEC-2026-0099.html)、[0049](https://rustsec.org/advisories/RUSTSEC-2026-0049.html)列出的 0.103.x 修复版本均低于锁定的 webpki 0.103.15。此项是定向检查，不代替全锁文件审计。 |
| 运行失败边界 | Redis 生产入口在 TLS 时检查 CPU，provider 冲突明确返回错误；普通 Redis URI 显式配置。本期未把 gRPC、SQLx、HTTPS 的隔离原型当作已接入最终节点。 |

## 采用决定

**采用该锁定组合继续 Windows x64 的 P1 无 C 技术路径及后续受控集成。**
这一决定限定于已列协议/证书用例、本机 CPU 和本地模拟/开发环境。
维护方的“项目较新”提示是已记录的现实风险；截至本次定向核对，未发现
可明确适用于锁定组合且尚未修复的严重公告，因此不把“缺少外部审计
报告”设为无限期等待条件。

本决定不批准实盘部署。P3–P8 接入后仍须核对统一 provider、实际特性并集、
选用的密钥交换组、证书来源、目标机器 CPU 和最终构建/链接；启用新 TLS
模式或部署环境时补其测试。任何新发现且适用的严重问题应暂停采用并
重新选型，不能回退到 AWS-LC/ring 或关闭证书验证来完成无 C 目标。
