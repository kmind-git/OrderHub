# P3：绑定、插件及通用示例裁剪记录

本批执行用户确认的建议：删除前四个通用示例目录，保留四个回测测试文件；
必要 Arrow 生成器迁移并验证后，再删除旧 persistence 示例目录。
本记录不表示 P3 已整体验收，也不表示研究依赖已完成最终无 C 化。

## 删除范围与恢复材料

- `examples/`：112 个文件。
- `crates/backtest/examples/`：4 个文件。
- `crates/common/examples/`：5 个文件。
- `crates/trading/src/examples/`：32 个文件。
- `crates/persistence/examples/`：1 个文件，生成器迁移验证后删除。

共删除上述五个目录中的 154 个文件。删除前逐文件验证 SHA-256 与
[`p3-source-before.zip`](p3-source-before.zip) 一致；路径、字节数及哈希见
[`example-deletion-preflight.json`](example-deletion-preflight.json)。
完整快照包含 2,742 个文件，快照哈希清单为
[`p3-source-before-hashes.json`](p3-source-before-hashes.json)。恢复时按仓库相对路径
提取需要的文件，检查当前修改后再恢复。

先前已分批删除 18 个 crate 的 `src/python/` 绑定实现、`crates/plugin/` SDK 和
`crates/live/src/node/plugin.rs` 宿主桩；消费者说明见
[`consumer-migration.md`](consumer-migration.md)。保留 Rust 业务实现和静态 FIX 适配器，
非空的旧动态插件配置由 LiveNode 显式拒绝。

## 保留的回测测试

保留 `ema_cross.rs`、`grid_mm.rs`、`grid_mm_itch.rs` 和 `book_imbalance.rs`。
EMA/Grid/Imbalance 实现迁至 `crates/backtest/tests/integration/workload_support/`，
仅供测试使用，不再由 trading crate 的通用 `examples` feature 导出。

[`workload-migration-parity.json`](workload-migration-parity.json) 对照迁移前后的四个集成
文件及三个夹具测试文件：63 个测试函数名称、147 条完整断言表达式均保持一致。

已运行以下验证，退出码均为 0：

- `cargo test --locked --offline -j 4 -p nautilus-backtest --test integration --profile nextest workload_support::`：54 个测试通过。
- 同一新构建集成测试二进制，过滤 `ema_cross::` 并排除 `workload_support::`：2 个通过，见 [日志](test-ema_cross.log)。
- 同一二进制，过滤 `grid_mm::` 并排除 `workload_support::`：3 个通过，见 [日志](test-grid_mm.log)。
- 同一二进制，过滤 `book_imbalance::` 并排除 `workload_support::`：1 个通过，见 [日志](test-book_imbalance.log)。

共 60 个实际测试通过。`nextest` Cargo profile 保留测试溢出检查和调试断言，关闭调试符号。
ITCH 的三个用例保留原有高精度条件；本机缺少其大型 Parquet 输入，未执行，不能计为通过。

## Arrow 生成器迁移

新入口：`crates/persistence/bin/generate_arrow_fixtures.rs`，二进制
`generate-arrow-fixtures`，需显式启用 `fixture-tools`。原有五份输入和转码逻辑保留，
新增 `--output-dir DIRECTORY`；不传参数时仍写入 `test_data/nautilus/arrow/`。

构建命令及退出码 0 见 [构建日志](generator-build.log)：

```text
cargo build --locked --offline -j 4 --profile nextest -p nautilus-backtest -p nautilus-persistence --features nautilus-persistence/fixture-tools --bin generate-arrow-fixtures --test integration
```

构建同时选择保留的集成目标，使依赖 feature 与回测验证复用。
随后运行新二进制，输出到 `target/arrow-fixture-verification/`，见
[运行日志](generator-run.log)。五份生成文件均与现有 Arrow 夹具逐字节一致，
SHA-256 对照见 [`generator-byte-comparison.json`](generator-byte-comparison.json)。
未替换已入库的生成文件，`depths.parquet` 不在此次生成范围。

缺失输出路径、未知选项、多余参数三组反例均以退出码 1 拒绝，见
[`generator-arguments.json`](generator-arguments.json)。
`test_data/nautilus/README.md` 已更新到新生成入口。

此生成器沿用既有 Zstd/Parquet 研究依赖。其消费者归入 P7/C4：最终移除 persistence
时核销，若必须保留则验证 Rust codec 的旧数据兼容和写入能力；本批 Rust 入口迁移
不替代最终构建、链接和运行时无 C 证据。

## 收尾检查

绑定删除留下的属性/文档空行已清理。一次性工具位于 `cfg-strip/src/bin/tidy_attributes.rs`，
逐文件比较清理前后完整 Rust AST token，确认只改变空白。
两个失效的宏 lint 预期标注已删除，未放宽 Clippy 规则。

- `cargo fmt --all -- --check`：退出码 0，见 [日志](format-check.log)。
- `git diff --check`：退出码 0，见 [日志](diff-check.log)。Git 的 CRLF/LF 提示不是检查失败。
- 标准精度工作区 `cargo check --locked --offline --workspace --all-targets --features nautilus-persistence/fixture-tools` 已通过；裁剪后的 `cargo clippy --locked --offline -j 4 --workspace --all-targets --features nautilus-persistence/fixture-tools -- -D warnings` 退出码 0，见 [日志](clippy.log)。最后工具修改后已重验同一配置。
- `cargo check --locked --offline -j 4 --workspace --all-targets --features high-precision,nautilus-backtest/streaming,nautilus-persistence/fixture-tools`：退出码 0，见 [日志](high-precision-check.log)。包括保留的 ITCH 与 streaming 测试目标，仅证明编译。
- `cargo test --locked --offline -j 4 -p nautilus-macros -p orderhub-tools --no-default-features --features orderhub-tools/checks --profile nextest --lib --bins`：6 个宏测试、7 个检查工具测试通过，见 [日志](macro-and-tool-tests.log)。

轻量 checks 配置的测试发现提交信息规则缺少 Regex Unicode 大小写功能，已在
`orderhub-tools` 的可选 Regex 依赖上显式启用 `unicode-case` 并重验通过。
Markdown 表格测试期望按原 Python 工具的同一中文/emoji 输入校正，未放宽生产规则。
这两个已验证样本不代表其余工具已完成全部迁移对照。

本批最终源文件 SHA-256 见 [`current-source-hashes.json`](current-source-hashes.json)，
验证二进制哈希见 [`verification-binary-hashes.json`](verification-binary-hashes.json)。
编译输出按先前磁盘清理要求清理；源码、恢复快照、日志、哈希和已打包 schema 工具保留。
已清理 `target/debug`、`target/nextest` 和临时 Arrow 输出目录，磁盘可用空间增加
31,331,446,784 字节，约 29.2 GiB；实测记录见
[`artifact-cleanup.json`](artifact-cleanup.json)。工具链为稳定版 Rust 1.98.1，
主机 `x86_64-pc-windows-msvc`，见 [`toolchain.txt`](toolchain.txt)。
五个旧目录均已不存在，四个测试文件和新生成器仍在，见
[`retirement-state.json`](retirement-state.json)。本批确认范围于 2026-10-01 完成收尾。

## P3 剩余项

- 其余必要 Python 工具的同输入对照：安全审计、提交信息、文档辅助、工作区测试清单等。
- shell 中嵌入式 Python、失效维护/发布辅助入口及其消费者。
- 通用 Python 源码、绑定生成脚本、PyO3 补丁目录；最后清理 Python 环境配置和锁文件。
- 相关 Make/pre-commit/CI/开发说明及历史文档引用的最终核销。
- 按保留消费者执行 P3 最终检查，记录最终文件哈希，再决定是否勾选 P3。

数据库与 Redis 的真实实例行为仍按既定启用清单后置，本批未连接这些服务。
没有提交、推送或发布代码。
