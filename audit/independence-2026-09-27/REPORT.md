# 本地独立化审计与验证记录（2026-09-27）

## 范围与归属

此 checkout 是为 OrderHub 独立维护的 NautilusTrader 本地派生版，仍保留原始版权、
LGPL-3.0-only、商标资料、Git 历史和内部 `nautilus-*` 名称。本轮不改项目名称；
`kmind-git/nautilus_trader` 是现有个人 fork 远程，不代表这些本地改动已经发布为独立
GitHub 项目。没有提交、推送、发布、部署、真实交易或更改 Git 历史。

父代理在本轮前将原 `origin` 地址备份到本目录的 `git-*-before.txt` 后移除；现在只剩
`fork` 远程。模板、CODEOWNERS、项目说明、安全报告入口及包元数据不再将此 checkout 的
维护路由指向 Nautech Systems。历史来源链接、版权声明、LICENSE 和 TRADEMARK 保留。
22 份现存 crate README 的旧上游 CI、docs.rs、crates.io 和 Discord badge 已移除，
以免本地派生版误显示上游发布状态或支持入口；正文中的来源链接仍保留。

## 可执行入口

- 原上游自托管、wheel、容器发布、夜间合并和文档派发工作流已退休；新增
  `.github/workflows/ci.yml`，在 GitHub-hosted Ubuntu 上执行 Rust 格式、整个 workspace
  的 all-targets check、OrderHub Clippy 和 `--lib --bins` 隔离测试，另设 Cargo.lock 的
  `cargo-audit` 依赖公告检查。此配置尚未在 GitHub 执行。
- `--lib --bins` 测试不启动 `m1_grpc` 与 `m2_local_e2e` 集成测试中硬编码的外部
  `rust-trader/exchange.exe`。`cargo check --workspace --all-targets` 只编译集成测试目标，
  不运行它们。外部端到端验证仍需单独规划和执行。
- `scripts/cli/install.sh` 与 `scripts/ci/install-nautilus-cli.sh` 只从本地源码构建 CLI。
  旧 wheel/Jupyter 应用镜像及 Makefile 的镜像 build/push 目标已退休。开发镜像与
  Redis/PostgreSQL compose 服务仍在；旧发布辅助脚本仍留作历史资料，没有活动工作流
  或 Make 发布目标调用。
- `.nautilus-engineering` 及锁文件留存来源和版本；pre-commit 不再强制与上游仓库同步。
  顶层 `nautilus-pyo3` wheel 构建与聚合 stub 生成不可用，但其它 crate 的可选 Python
  接口及 Python 源码未当作本轮全部删除。
- `nautilus-network` 与顶层 `nautilus-pyo3` 已从 workspace 移除；`nautilus-live`、
  `nautilus-testkit`、OrderHub、Redis 和 PostgreSQL 能力保留。AWS-LC 整体替换未实施。
  详见 `orderhub-tls-stack-removal.md`。

## 验证

- `git diff --check`：通过。
- stable `cargo fmt --all -- --check` 在本机失败，因为 rustfmt.toml 需要 nightly 选项；
  新 CI 改为安装 `tools.toml` 指定的 `nightly-2026-08-21` 再检查。此主机尚未安装该
  nightly，因此新格式门禁仍待验证。父代理在格式化本轮修改后，用本机 rustfmt 1.98.1
  对 `recovery.rs`、`socket.rs`、`files.rs` 三个文件执行局部
  `rustfmt --check --edition 2024` 已通过；此结果不覆盖 nightly 配置或全部文件。
- Cargo.toml、python/pyproject.toml 的 TOML 解析，以及新 CI 与 pre-commit YAML 解析：
  通过；这仅证明结构可解析，不等于 GitHub Actions 实际运行通过。
- `.pre-commit-hooks/check_dependency_features.py` 与对应测试：通过。调整的策略只删除
  已不在当前 workspace 的依赖/消费者约束，同时纳入 OrderHub gateway 的现有
  `test-support` dev 依赖。
- 原用户改动的 `crates/persistence/Cargo.toml` SHA-256 仍为
  `BB08A72B6E52A27BADDC629B8F8EB4C3A0B1DA411868DAFB0D58B8F9A2CEB122`；
  `docs/concepts/data/index.md` 仍为
  `7FE9F0745C900E0B0F8E88C4BF44DB1747FB7D4F4AE38E53E554952813AB1686`。
- 父代理在 `CARGO_BUILD_WARNINGS=warn` 诊断设置下运行：testkit `files::tests`
  26/26、live `book::recovery::tests` 6/6、live `socket::tests` 15/15 通过。
  同样设置下，`cargo test --locked --offline -p orderhub-gateway -p orderhub-node
  -p orderhub-proto --lib --bins` 中 gateway 6/6 通过，node/client/proto 测试目标
  成功（这些目标共 0 项测试），未运行依赖外部 exchange 的集成测试。
  标准命令在此 Windows 主机因链接器中文输出触发 `build.warnings=deny` 而失败；
  这些诊断结果不能记为标准门禁通过。
- `cargo check --locked --offline --workspace --all-targets`：通过。首次运行发现
  `crates/cli/tests/exit_code.rs` 仍引用 CLI 已不存在的 `defi` feature 与
  `blockchain` 命令；删除这份失效测试并更正 CLI README 后，同一命令重跑通过。
- `cargo check --locked --offline -p nautilus-infrastructure --features redis,postgres`：
  通过，证明两项功能可以同时编译；未连接实际服务。
- `cargo clippy --locked --offline -p orderhub-gateway -p orderhub-node -p orderhub-proto
  --all-targets -- -D warnings`：标准命令仍因 Windows 链接器中文输出被
  `build.warnings=deny` 拦下；仅将 `CARGO_BUILD_WARNINGS=warn` 用于此单次诊断后，
  Clippy 通过。不能记为标准门禁通过。
- 受限执行环境初次启动 Git Bash 报 `couldn't create signal pipe, Win32 error 5`。
  经只读命令的自动权限审查后，`bash -n` 对 `scripts/cli/install.sh`、
  `scripts/ci/install-nautilus-cli.sh`、`scripts/clippy-changed.sh`、
  `scripts/doc-changed.sh`、`scripts/test-check-cargo-cooldown.bash`、
  `scripts/ci/check-docker-toolchain-pins.bash`、`scripts/curate-dataset.sh` 和
  `scripts/ci/check-generated-drift.bash` 的语法检查通过；Docker toolchain pin 检查
  脚本本地运行通过。`scripts/test-check-cargo-cooldown.bash` 的功能测试尝试因 Git Bash
  中 `python3` 指向不可用的 Windows Store alias 而中止，没有得到测试结论；临时目录
  已清理。其它 Bash 功能脚本及 `make test-scripts` 尚未完整执行。
  GitHub-hosted CI、cargo-audit、Redis/PostgreSQL 服务集成、外部 exchange 和完整
  Python 包测试均未在本轮验证。

## 尚待验证

这个本地开发 checkout 还需在 GitHub-hosted runner 上实际执行新 CI，确认格式、
workspace check、OrderHub 测试与依赖公告检查。Redis/PostgreSQL 集成测试需要显式的
本地服务环境；Windows 链接器警告门禁的问题也需单独处理。本轮没有生产部署或实盘
验收目标，旧 upstream 发布辅助脚本不应作为此派生版的发布流程。
