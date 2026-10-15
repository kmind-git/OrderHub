# CentOS Stream 9 部署

本发布包面向 **CentOS Stream 9 / x86_64**，在官方 Stream 9 容器内使用仓库固定的
Rust 1.98.1 构建 `release`，启用 `grpc-tls`。包内有节点、联调客户端、离线恢复工具、
部署模板、构建标识、SHA-256 清单和对应提交的源码归档。服务器无需安装 Rust。
Redis/PostgreSQL 服务保持未启用。报价合并仍为讨论方案，本包使用已提交的逐条输入实现。

## 获取与验证

在 [CentOS 发布工作流](https://github.com/kmind-git/OrderHub/actions/workflows/centos9-release.yml)
选择 `master` 并执行 Run workflow。只有依赖审计、格式、检查、Clippy、业务/恢复/TLS 测试、
release 构建和解包冒烟均成功，才上传 `orderhub-centos9-x86_64-完整提交SHA` artifact。
Actions artifact 保留 90 天；持久发布资产以仓库 Releases 中实际上传的包为准。
取出其中 `.tar.gz` 和同名 `.sha256`，保留 `verification` 构建证据。

下载并解开 Actions artifact 的 ZIP 后，`dist/` 下是发布压缩包和校验文件；
`smoke/orderhub-*/verification/` 下是解包冒烟的 TLS、退出、备份恢复和 FIX event 日志。
冒烟记录在压缩包生成后产生，单独保存在 artifact 中。发布压缩包解开后包含：

```text
orderhub-centos9-x86_64-提交前12位/
  bin/          orderhub-node、orderhub-client、orderhub-recovery
  deploy/       orderhub.service、orderhub.env.example、orderhub-fix.cfg
  docs/         部署、TLS、输入和恢复说明
  verification/ 二进制类型、动态依赖和符号版本检查
  BUILD-INFO.txt、SHA256SUMS、Cargo.lock、SOURCE-NOTICES.txt、source.tar.gz
```

CPU 必须提供 `aes ssse3 avx avx2 adx bmi2 pclmulqdq`，虚拟机须暴露这些指令。
这是当前 TLS 后端额外要求；仅能运行 Stream 9 并不能保证满足要求。
构建不会启用 `target-cpu=native`。部署前查看 `lscpu`，并在目标主机执行
`ldd`，确认没有 `not found`。包中记录各二进制的动态依赖与符号版本，拒绝 glibc > 2.34。
服务器保持 SELinux 启用；若发生拒绝，查看 AVC 并按实际路径处理，避免关闭 SELinux。

## 管理员首次准备

以下命令由 root 执行一次。运行与发布更新由 `orderhub` 账号完成，不授予其通用 sudo。

```bash
dnf install -y tar gzip coreutils glibc libgcc libstdc++ openssl \
  systemd systemd-pam dbus dbus-daemon util-linux policycoreutils
useradd --create-home --home-dir /home/orderhub --shell /bin/bash orderhub
passwd -l orderhub
loginctl enable-linger orderhub
systemctl start "user@$(id -u orderhub).service"
```

若账号已存在，先确认 `getent passwd orderhub` 的 home 和 shell，跳过 `useradd`。
按现有服务器策略配置该用户的 SSH 公钥，或由管理员 `sudo -iu orderhub` 切换。
linger 让用户服务在退出 SSH 后持续运行，并在主机启动时建立用户服务管理器。

## orderhub 用户安装

以下命令以 `orderhub` 用户执行；示例 home 为 `/home/orderhub`。先将下载包及校验文件
放入自己的 `~/downloads`。所有发布目录、配置和业务状态都归 `orderhub`。

```bash
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
umask 077
mkdir -p ~/downloads ~/orderhub/releases ~/orderhub/config/tls \
  ~/orderhub/state/checkpoints ~/orderhub/backups ~/orderhub/run ~/orderhub/logs/fix \
  ~/.config/systemd/user
cd ~/downloads
# 用实际文件名替换；同名 .sha256 文件必须同时存在
archive=orderhub-centos9-x86_64-实际提交前12位.tar.gz
sha256sum -c "$archive.sha256"
tar -xzf "$archive" -C ~/orderhub/releases
release="${archive%.tar.gz}"
cd "$HOME/orderhub/releases/$release"
sha256sum -c SHA256SUMS
ldd bin/orderhub-node
ln -s "releases/$release" ~/orderhub/current
cp deploy/orderhub.service ~/.config/systemd/user/orderhub.service
cp deploy/orderhub.env.example ~/orderhub/config/orderhub.env
cp deploy/orderhub-fix.cfg ~/orderhub/config/orderhub-fix.cfg
chmod 600 ~/orderhub/config/orderhub.env ~/orderhub/config/orderhub-fix.cfg
```

`current` 已存在时按后文更新流程操作。确认 `BUILD-INFO.txt` 的提交、feature 和目标正确。
包中的 `orderhub-client` 会尝试提交测试订单，**不能作为普通线上健康检查命令**。
发布冒烟在隔离环境中通过关闭新订单门禁，只执行它的 TLS GetState 前置步骤。

安装你选定 CA 签发的服务器证书链 `server-chain.pem` 和 PKCS#8 私钥 `server-key.pem`
到 `~/orderhub/config/tls/`；设置私钥权限 600。证书 SAN 必须匹配客户端配置的域名。
包中不提供部署用私钥或通用测试证书。TLS 配置见同目录 `tls.md` 或仓库 TLS 文档。
发布版即使监听本机回环地址也要求 TLS；调用方需要受信 CA、匹配的服务器域名和
`Authorization: Bearer <token>`。`orderhub-client` 对应使用 `ORDERHUB_TLS_CA`、
`ORDERHUB_TLS_DOMAIN`、`ORDERHUB_TOKEN` 和 `ORDERHUB_ADDR`，不要把服务端私钥分发给调用方。

生成应用身份注册表，示例当前本地 profile 使用 `trader-1 / ORDERHUB-S001`：

```bash
token="$(openssl rand -hex 32)"
printf '{"submitters":[{"token":"%s","submitter_id":"trader-1","strategies":["ORDERHUB-S001"]}]}\n' \
  "$token" > ~/orderhub/config/submitters.json
unset token
chmod 600 ~/orderhub/config/submitters.json
```

编辑 `orderhub.env` 的路径和监听地址，以及 `orderhub-fix.cfg` 中的 FIX 地址、CompID 和心跳。
`ORDERHUB_FIX_CONFIG=/home/orderhub/orderhub/config/orderhub-fix.cfg` 指向该文件；
不再设置 `ORDERHUB_EXCHANGE_ADDR`。环境文件不执行 shell 展开，
路径必须写绝对路径，不能写 `$HOME`。默认 `127.0.0.1:50051`；需要远程访问时改成
明确的监听 IP，并由管理员按来源范围配置防火墙。当前执行适配器是本地 FIX 场所适配器，
尚未提供通用券商/实盘执行接入；默认地址并不意味着发布包包含模拟交易所。
TLS 保护客户端到节点的 gRPC 链路。当前 FIX 适配器使用普通 TCP，默认连接本机
`127.0.0.1`；跨机 FIX 链路的保护由部署网络提供。

默认 `FileLogPath=../logs/fix` 会写入 `/home/orderhub/orderhub/logs/fix/`，
其中 `FIX.4.2-ORDERHUB-GOX.messages.current.log` 为收发报文，
`FIX.4.2-ORDERHUB-GOX.event.current.log` 为会话事件。每类默认 10 MiB + 5 个历史文件。
日志设置和失败策略见同目录 `local-exchange.md`。配置修改后由 `orderhub` 用户重启服务。

## 由 orderhub 用户启停

```bash
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
systemctl --user daemon-reload
systemctl --user enable orderhub.service
systemctl --user start orderhub.service
systemctl --user status orderhub.service --no-pager
journalctl --user -u orderhub.service -n 100 --no-pager

systemctl --user stop orderhub.service
```

主机重启后由 linger 和用户 unit 的 `default.target` 启动服务。`Restart=no` 不循环重启
失败实例。`stop` 向进程发送 SIGINT，触发现有关闭新风险、排空回报、刷盘和检查点流程。
日志应包含 `stopped; checkpoint=...`，并检查 `systemctl --user show orderhub.service
-p Result -p ExecMainStatus`；正常值为 `success / 0`。60 秒内未退出时 systemd 可能强杀，
须按异常恢复处理。避免 `kill -9`；不使用 SIGTERM 代替当前仅实现的 Ctrl+C 路径。

进程 `active` 只表示服务在运行。没有当前合法的合约、报价、账户、状态输入或执行连接时，
新订单门禁保持关闭。由受权客户端只调用 GetState 检查 `readiness`、`writable`、
`persistence_mode`、`state_cursor` 与 `durable_cursor`，不要用实际下单测试替代健康检查。

## 接入当前 stdin 输入入口

默认 `ORDERHUB_INPUT_STDIN=0`，服务可用于诊断，不能据此开放下单。当前入口只接受
版本化本地 fixture JSONL，契约见 `input-runtime.md`；它不自动查询外部账户或生成心跳。
如果已准备持续输出合法 JSONL 的可信生产者，停止服务，设置 env 的
`ORDERHUB_INPUT_STDIN=1`，并以 `orderhub` 用户配置：

```bash
mkfifo -m 600 ~/orderhub/run/input.fifo
mkdir -p ~/.config/systemd/user/orderhub.service.d
cat > ~/.config/systemd/user/orderhub.service.d/input.conf <<'UNIT'
[Service]
StandardInput=file:%h/orderhub/run/input.fifo
UNIT
systemctl --user daemon-reload
```

只在 FIFO 尚不存在时运行 `mkfifo`。从另一终端启动你的实际生产者，将其输出重定向到
`~/orderhub/run/input.fifo`，再启动服务；FIFO 两端连接前可能等待。不要以读写方式
永久占住 FIFO 来隐藏 EOF，也不要循环回放过期文件冒充当前账户/状态。
生产者退出导致 EOF，输入运行时失效并关闭新风险；恢复输入需要重启节点和生产者，
并重新建立当前完整快照。生产者的启停顺序由其自己的服务配置负责。

## 更新与备份

更新仍由 `orderhub` 用户执行。先校验并解包新版本到新的 releases 目录，停止服务并确认
退出成功，随后备份原库。不要在运行时替换二进制或复制正在写入的 redb 文件。

```bash
systemctl --user stop orderhub.service
~/orderhub/current/bin/orderhub-recovery inspect ~/orderhub/state/orderhub-journal.redb
~/orderhub/current/bin/orderhub-recovery backup \
  ~/orderhub/state/orderhub-journal.redb "$HOME/orderhub/backups"
```

记录输出的 manifest 路径，并把备份复制到另一存储设备。
切换前完成配置迁移：

- 从仍使用 `ORDERHUB_EXCHANGE_ADDR` 的旧版本升级时，把地址拆为
  `orderhub-fix.cfg` 的 `SocketConnectHost` 和 `SocketConnectPort`，核对双方 CompID 和心跳。
  在现有 `orderhub.env` 中删除旧变量，加入
  `ORDERHUB_FIX_CONFIG=/home/orderhub/orderhub/config/orderhub-fix.cfg`。
- 如果实例尚无 FIX 配置，先从新包复制 `deploy/orderhub-fix.cfg` 到
  `~/orderhub/config/orderhub-fix.cfg`，权限设为 600，再按上述步骤修改。
  已有配置、Token 注册表、TLS 证书和业务库继续保留；不要用示例 env 覆盖现有配置。
- 确认 `~/orderhub/logs/fix` 存在且归 `orderhub`，并留有轮转日志所需空间。

随后使用新目录名更新用户 unit 并原子切换 `current`；自定义 unit 设置放在现有
`orderhub.service.d/` 下的 drop-in 中：

```bash
new_release=orderhub-centos9-x86_64-新提交前12位
cd ~/orderhub
cp "releases/$new_release/deploy/orderhub.service" ~/.config/systemd/user/orderhub.service
systemctl --user daemon-reload
ln -s "releases/$new_release" current.next
mv -Tf current.next current
systemctl --user start orderhub.service
```

恢复工具 `restore MANIFEST NEW-JOURNAL` 要求新库不存在，恢复后设置新的 epoch 和对账栅栏。
当前包含任何历史订单的库、备份恢复库或异常异步会话重启都可能保持 HALTED，
即使旧订单全部终态也不会自动恢复交易。发布包不包含解除该门禁的自动对账功能。
因此更新后的服务可启动不等于可以立即恢复下单；执行端核对和接管须遵循现有恢复契约。
更换程序版本时保留原业务库和备份，不通过删除库解决 HALTED。

默认 `async` ACK 可能先于刷盘，异常断电可能丢失已 ACK 的尾部；5 ms 为检查周期，
不是最大数据丢失窗口。需要同步确认时设置 `ORDERHUB_PERSISTENCE=sync` 并正常重启。
报价只在内存，重启须从当前行情源重新取得。更多细节见 `persistence-policy.md`。

## 验证边界

GitHub 流程验证 Stream 9 容器用户态中的构建、动态依赖、TLS 请求、正常 SIGINT 退出及
离线备份恢复，并验证用户 unit 的语法。容器共用 GitHub runner 的内核，未运行完整
CentOS 启动过程、真实用户 systemd 会话、SELinux 策略、真实 FIX 场所或断电实验。
目标服务器首次部署仍需验证 CPU、证书、权限、用户会话、启停日志及只读 GetState。

官方参考：[CentOS Stream 9](https://www.centos.org/stream9/)、
[systemd kill 配置](https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html)、
[loginctl linger](https://www.freedesktop.org/software/systemd/man/latest/loginctl.html)。
