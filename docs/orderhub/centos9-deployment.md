# OrderHub 部署

适用：CentOS Stream 9 / x86_64，以 `orderhub` 用户部署到 `/home/orderhub`。
发布版使用固定 stable Rust 构建，启用 `grpc-tls`；服务器无需安装 Rust。

## 1. 发布包结构

在仓库的 **OrderHub CentOS Stream 9 release** 工作流选择 `master` 构建。
下载 Actions artifact，取出 `dist/` 下的发布包与同名 `.sha256`。
`orderhub-centos9-x86_64-提交前12位.tar.gz` 中直接包含以下结构，没有版本目录外壳：

```text
bin/
  orderhub-node
  orderhub-client
  orderhub-recovery
config/
  orderhub.env
  orderhub-fix.cfg
  tls/                         # 空目录，放已有证书和私钥
logs/
  fix/                         # 空目录，运行后生成 FIX 日志
systemd/
  orderhub.service
  orderhub.service.d/           # 空目录，放实例的补充配置
deployment.md
BUILD-INFO.txt
SHA256SUMS
SOURCE-NOTICES.txt
```

解压到 `/home/orderhub` 就是最终位置，不需要复制程序、移动配置或修改模板路径。
unit 首次启动时创建 `data/checkpoints/`，业务库为 `data/orderhub-journal.redb`；
发布包不携带业务库、Token、证书或私钥。

同一 artifact 的 `dist/*-source.tar.gz` 保存对应提交的完整源码、Cargo.lock 和其他文档，
各有独立 `.sha256`。`dist/verification/` 和 `smoke-verification/` 保存构建、冒烟记录。
源码包不解压到部署目录。Actions artifact 保留 90 天。

## 2. 环境检查

以 `orderhub` 用户执行：

```bash
command -v tar gzip sha256sum openssl systemctl loginctl ldd
loginctl show-user orderhub -p Linger
lscpu
```

CPU 需要 `aes ssse3 avx avx2 adx bmi2 pclmulqdq`，运行库为 CentOS 9 的 glibc 2.34。
工具齐全、账号可写家目录且 `Linger=yes`，即可继续，无需 sudo。
缺工具或账号时由管理员准备；`Linger=no` 时管理员执行一次
`loginctl enable-linger orderhub`。保持 SELinux 启用。

## 3. 首次解压

已有节点或 `config/orderhub.env` 时使用第 6 节更新步骤。
先把发布 `.tar.gz` 和同名 `.sha256` 上传到 `/home/orderhub/downloads/`。
以下以 `orderhub` 用户执行，文件名替换为实际提交：

```bash
set -euo pipefail
umask 077
cd /home/orderhub/downloads
archive=orderhub-centos9-x86_64-实际提交前12位.tar.gz
sha256sum -c "$archive.sha256"
test ! -e /home/orderhub/bin/orderhub-node
test ! -e /home/orderhub/config/orderhub.env
tar --no-same-owner --keep-old-files -xzf "$archive" -C /home/orderhub
cd /home/orderhub
sha256sum -c SHA256SUMS
ldd bin/orderhub-node
ldd bin/orderhub-client
ldd bin/orderhub-recovery
```

`ldd` 不应出现 `not found`。已有 `config/tls/` 内文件会保留；包内该目录为空。

### 文件位置与手工配置清单

| 最终绝对路径                                         | 来源              | 手工操作                                                             |
| ---------------------------------------------------- | ----------------- | -------------------------------------------------------------------- |
| /home/orderhub/bin/orderhub-node                     | 发布包            | 无需移动，由 systemd 启动                                            |
| /home/orderhub/config/orderhub.env                   | 发布包            | 核对 gRPC 监听地址；需要远程连接时设置 `ORDERHUB_ADDR=0.0.0.0:50051` |
| /home/orderhub/config/orderhub-fix.cfg               | 发布包            | 设置交易所 IP、端口、SenderCompID、TargetCompID 和心跳               |
| /home/orderhub/config/submitters.json                | 首次生成或复用    | 保存 Token、提交方及策略权限，生成命令见下文                         |
| /home/orderhub/config/client-token.txt               | 首次生成或复用    | 安全交给客户端；节点读取 submitters.json，不读取本文件               |
| /home/orderhub/config/tls/server-chain.pem           | 自行提供或复用    | 服务器证书链，证书 SAN 与客户端验证名称一致                          |
| /home/orderhub/config/tls/server-key.pem             | 自行提供或复用    | 与服务器证书匹配的 PKCS#8 私钥，权限 600                             |
| /home/orderhub/config/tls/ca.pem                     | 自行提供或复用    | CA 公钥证书，用于验证和分发给客户端；节点不读取本文件                |
| /home/orderhub/systemd/orderhub.service              | 发布包            | 默认路径无需修改，按第 4 节注册用户服务                              |
| /home/orderhub/systemd/orderhub.service.d/input.conf | 接入 stdin 时创建 | 指定真实输入管道；仅在已准备持续生产者时配置                         |

`orderhub-client` 和 `orderhub-recovery` 也位于 `/home/orderhub/bin/`。
TLS 三个文件全部放在 `/home/orderhub/config/tls/`；现有文件直接复用，不上传 CA 私钥。

应用环境文件的路径默认已经正确，无需再次逐项改路径：

```ini
ORDERHUB_FIX_CONFIG=/home/orderhub/config/orderhub-fix.cfg
ORDERHUB_SUBMITTERS=/home/orderhub/config/submitters.json
ORDERHUB_TLS_CERT=/home/orderhub/config/tls/server-chain.pem
ORDERHUB_TLS_KEY=/home/orderhub/config/tls/server-key.pem
ORDERHUB_JOURNAL=/home/orderhub/data/orderhub-journal.redb
ORDERHUB_CHECKPOINT_DIR=/home/orderhub/data/checkpoints
```

上述业务库路径用于首次部署；已有实例按第 6 节保留或完整迁移原库。
`ORDERHUB_PERSISTENCE=async` 可保持默认。`ORDERHUB_INPUT_STDIN=0` 表示没有 stdin 输入；
接入持续生产者及 FIFO 后再改为 `1`，仅修改该值不会自动生成账户、合约或行情。

FIX 文件需要核对的内容如下，同机模拟交易所示例为：

```ini
SocketConnectHost=127.0.0.1
SocketConnectPort=5001
SenderCompID=ORDERHUB
TargetCompID=GOX
HeartBtInt=30
FileLogPath=../logs/fix
```

在已有文件所属的 DEFAULT/SESSION 段中修改对应项，保留其余设置，不用上述片段覆盖整个文件。
FIX 日志最终写入 `/home/orderhub/logs/fix/`。模拟交易所不在同机时，换成其实际地址。

同机模拟交易所使用 `SocketConnectHost=127.0.0.1`、端口 `5001`，并先启动交易所。
环境文件路径使用绝对路径，不写 `$HOME`；其他家目录须修改 env 中的 `/home/orderhub`。
不再设置已退役的 `ORDERHUB_EXCHANGE_ADDR`。

将已有 `server-chain.pem`、`server-key.pem`、`ca.pem` 放在 `config/tls/`。
首次生成应用凭证，已有注册表则复用：

```bash
set -euo pipefail
umask 077
cd /home/orderhub/config
if [ ! -e submitters.json ]; then
  (
    set -o noclobber
    token="$(openssl rand -hex 32)"
    printf '%s\n' "$token" > client-token.txt
    printf '{"submitters":[{"token":"%s","submitter_id":"trader-1","strategies":["ORDERHUB-S001"]}]}\n' \
      "$token" > submitters.json
  )
fi
chmod 700 tls
chmod 600 orderhub.env orderhub-fix.cfg submitters.json tls/server-key.pem
test -r tls/server-chain.pem
test -r tls/ca.pem
```

客户端取得 CA、证书验证名称和 Token；Token 不写入聊天或部署日志。
例如现有证书名称为 `orderhub.test`，客户端仍连接服务器 IP，但验证该 DNS 名称。

## 4. 注册和启停

首次注册用户服务，unit 和 drop-in 实体均保留在 `/home/orderhub/systemd/`：

```bash
set -euo pipefail
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
mkdir -p /home/orderhub/.config/systemd/user
ln -s /home/orderhub/systemd/orderhub.service /home/orderhub/.config/systemd/user/orderhub.service
ln -s /home/orderhub/systemd/orderhub.service.d /home/orderhub/.config/systemd/user/orderhub.service.d
systemctl --user daemon-reload
systemctl --user show orderhub.service --no-pager -p EnvironmentFiles -p DropInPaths -p WorkingDirectory -p ExecStart
systemctl --user start orderhub.service
systemctl --user status orderhub.service --no-pager -l
journalctl --user -u orderhub.service -n 50 --no-pager
```

如果已有注册文件，先核对并备份后调整链接；现有 drop-in 应迁到 `systemd/orderhub.service.d/`。
不要保留指向旧 `config/systemd/` 或 `orderhub/current/` 的链接。
确认模拟交易所、输入生产者的启动顺序后，再执行 `systemctl --user enable orderhub.service`。

需要停止时单独执行：

```bash
systemctl --user stop orderhub.service
systemctl --user show orderhub.service --no-pager -p Result -p ExecMainStatus
```

正常值为 `success / 0`，节点日志应有 `stopped; checkpoint=...`。
unit 使用 SIGINT，停止超时 60 秒；`Restart=no`。异常退出按恢复流程处理。

## 5. 检查和业务输入

安全组和防火墙按客户端来源允许 TCP 50051。检查 TLS 和 FIX：

```bash
openssl verify -CAfile /home/orderhub/config/tls/ca.pem -purpose sslserver \
  -verify_hostname orderhub.test /home/orderhub/config/tls/server-chain.pem
ss -lntp '( sport = :50051 )'
tail -n 20 /home/orderhub/logs/fix/FIX.4.2-ORDERHUB-GOX.event.current.log
```

证书名称应换成实际 SAN。FIX 日志应有登录成功和双向心跳；`messages.current.log`
记录收发报文，`event.current.log` 记录会话事件，每类默认 10 MiB 加 5 个历史文件。
TLS 保护 gRPC；FIX 为普通 TCP，跨机保护由部署网络提供。

服务运行、TLS 及 FIX 正常不等于业务可下单。默认 `ORDERHUB_INPUT_STDIN=0`，
当前仍需合约、合法报价、账户和交易状态输入，再以只读 GetState 确认 `writable=true`。
包内 `orderhub-client` 会自动发单，不用于健康检查。

使用已有 JSONL 生产者时，停机后设置 `ORDERHUB_INPUT_STDIN=1`，执行
`mkdir -p /home/orderhub/data`，仅在 FIFO 尚不存在时执行
`mkfifo -m 600 /home/orderhub/data/input.fifo`，并添加 `systemd/orderhub.service.d/input.conf`：

```ini
[Service]
StandardInput=file:%h/data/input.fifo
```

以 `orderhub` 用户把生产者输出送到 FIFO，再启动节点；生产者必须持续运行。
首次 FIFO 两端连接前可能等待。EOF 会关闭输入门禁，不循环回放过期账户/状态。
这仍是现有本地 fixture 入口，独立数据 API 尚未实现；FIX 适配器不负责账户查询。

## 6. 已有实例更新

先校验新包，正常停止服务并确认 `success / 0`，用旧版 `orderhub-recovery backup`
备份 **当前 env 指向的原业务库**，并备份 `config/`、现有 unit、drop-in 和旧程序。
将现有 drop-in 完整迁到 `/home/orderhub/systemd/orderhub.service.d/`，保留其内容。
随后只更新程序、unit、注册链接和发布说明：

```bash
set -euo pipefail
tar --no-same-owner --no-overwrite-dir --exclude=config --exclude=logs \
  -xzf /home/orderhub/downloads/实际发布包.tar.gz -C /home/orderhub
cd /home/orderhub
grep -E ' [ *]\./bin/' SHA256SUMS | sha256sum -c -
ln -sfnT /home/orderhub/systemd/orderhub.service /home/orderhub/.config/systemd/user/orderhub.service
ln -sfnT /home/orderhub/systemd/orderhub.service.d /home/orderhub/.config/systemd/user/orderhub.service.d
systemctl --user daemon-reload
```

配置、TLS、Token、FIX 地址和日志继续保留；保留配置后不使用整包 SHA256SUMS 检查配置。
核对 unit 注册链接及 drop-in 后再启动。旧布局的数据库可能仍在 `deploy/state/` 或
`orderhub/state/`；保持 env 指向原库。若要迁到 `data/`，必须停机后完整迁移并修改路径，
不能把路径改到空目录来创建新库。FIX 日志路径可改为 `/home/orderhub/logs/fix`。

有历史订单、备份恢复或异常 async 会话时，重启可能保持 HALTED，需要外部对账；
不能删库解除门禁。async ACK 可能先于刷盘；报价仅在内存，重启后需要重新输入。
Redis/PostgreSQL 未启用。

GitHub 冒烟覆盖 CentOS 9 容器用户态的解包、TLS、SIGINT、备份恢复及 unit 语法，
未验证完整主机启动、真实用户 systemd 会话、SELinux、真实 FIX 场所或断电场景。
