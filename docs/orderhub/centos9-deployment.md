# OrderHub 部署

适用：CentOS Stream 9 / x86_64，以 `orderhub` 用户部署到 `/home/orderhub`。
发布版使用固定 stable Rust 构建，启用 `grpc-tls`；服务器无需安装 Rust。

## 1. 人工解压后交给 agent

人工下载运行发布包及同名 `.sha256`，校验后以 `orderhub` 用户解压到 `/home/orderhub`。
打开解压后的 `/home/orderhub/deployment.md`，让远端 agent 从第 2 节开始执行。
agent 无需再次下载、解压或搬运文件，也不依赖压缩包仍然存在。
运行包直接包含以下结构，没有版本目录外壳：

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

若人工解压在其他位置，先调整到上述最终位置再交给 agent。
发布包不携带业务库、Token、证书或私钥。TLS 已有文件应保留。
已有部署必须在解压前按第 6 节停机、备份并保护配置和业务库，不能直接覆盖运行中的实例。

同一 artifact 的 `dist/*-source.tar.gz` 保存对应提交的完整源码、Cargo.lock 和其他文档，
各有独立 `.sha256`。`dist/verification/` 和 `smoke-verification/` 保存构建、冒烟记录。
源码包不解压到部署目录。Actions artifact 保留 90 天。

## 2. 检查环境和解压结果

以下命令均以 `orderhub` 用户在 Bash 中执行。每个命令块成功后再执行下一块。
先用存在性检查确认路径；缺必需文件时报告并暂停该步骤，不能继续 `cd`、读取或修改它。
可创建的目录先创建；可选日志、管道、业务库尚未生成时按下文处理，不盲目探测旧路径。

```bash
set -euo pipefail
umask 077
if [ "$(id -un)" != orderhub ]; then
  echo '请以 orderhub 用户执行' >&2
  exit 1
fi
for tool in sha256sum openssl systemctl journalctl loginctl ldd lscpu ss readlink; do
  if ! command -v "$tool" > /dev/null; then
    printf '缺少工具，请由管理员安装：%s\n' "$tool" >&2
    exit 1
  fi
done
if [ ! -d /home/orderhub ]; then
  echo '请先完成人工解压到 /home/orderhub' >&2
  exit 1
fi
cd /home/orderhub
for path in bin/orderhub-node bin/orderhub-client bin/orderhub-recovery \
  config/orderhub.env config/orderhub-fix.cfg systemd/orderhub.service SHA256SUMS; do
  if [ ! -s "$path" ] || [ ! -r "$path" ]; then
    printf '缺少或无法读取发布文件：%s/%s\n' "$PWD" "$path" >&2
    exit 1
  fi
done
mkdir -p config/tls logs/fix systemd/orderhub.service.d data/checkpoints
grep -E ' [ *]\./bin/' SHA256SUMS | sha256sum -c -
ldd bin/orderhub-node
ldd bin/orderhub-client
ldd bin/orderhub-recovery
loginctl show-user orderhub -p Linger
lscpu
```

CPU 需要 `aes ssse3 avx avx2 adx bmi2 pclmulqdq`，运行库为 CentOS 9 的 glibc 2.34。
工具齐全、账号可写家目录且 `Linger=yes`，即可继续，无需 sudo。
`ldd` 不应出现 `not found`。缺工具或账号时由管理员准备；`Linger=no` 时管理员执行一次
`loginctl enable-linger orderhub`。保持 SELinux 启用。
这里只核验程序哈希，避免已有手工配置导致整包校验失败。
刚解压且尚未修改任何包内文件时，也可在该目录执行 `sha256sum -c SHA256SUMS`。

## 3. 准备配置和凭证

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

### TLS 和应用凭证

先确认已有 `server-chain.pem`、`server-key.pem`、`ca.pem` 已放到 `config/tls/`。
以下检查只判断文件是否存在，缺失时明确提示，补齐后再继续，不对缺失文件执行读取或 chmod：

```bash
set -euo pipefail
umask 077
if [ ! -d /home/orderhub/config ]; then
  echo '请先完成第 2 节检查' >&2
  exit 1
fi
cd /home/orderhub/config
mkdir -p tls
for path in orderhub.env orderhub-fix.cfg tls/server-chain.pem tls/server-key.pem tls/ca.pem; do
  if [ ! -s "$path" ] || [ ! -r "$path" ]; then
    printf '请先补齐可读取的配置文件：%s/%s\n' "$PWD" "$path" >&2
    exit 1
  fi
done
openssl verify -CAfile tls/ca.pem -purpose sslserver \
  -verify_hostname orderhub.test tls/server-chain.pem
if [ -e submitters.json ] || [ -L submitters.json ]; then
  if [ ! -s submitters.json ] || [ ! -r submitters.json ]; then
    echo '已有 submitters.json 为空或不可读，请修复后继续' >&2
    exit 1
  fi
elif [ -e client-token.txt ] || [ -L client-token.txt ]; then
  echo '已有 Token 但缺少注册表，请恢复匹配的 submitters.json 后继续' >&2
  exit 1
else
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
if [ -f client-token.txt ]; then
  chmod 600 client-token.txt
fi
```

验证命令中的 `orderhub.test` 应与实际证书 SAN 一致，名称不同先替换再执行。
已有注册表直接复用，不重新生成 Token；`client-token.txt` 缺失不影响节点启动，
客户端凭证须由维护者安全提供，不能因此重置注册表。Token 不写入聊天或部署日志。
客户端取得 CA、证书验证名称和 Token；连接服务器 IP 时仍验证证书中的 DNS 名称。

## 4. 注册和启停

unit 和 drop-in 实体均保留在 `/home/orderhub/systemd/`。
先完成第 2、3 节；首次注册或重复执行时，以下命令只创建缺失链接。
若发现旧注册项，先按第 6 节处理，不能覆盖现有 drop-in：

```bash
set -euo pipefail
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
if [ ! -S "$XDG_RUNTIME_DIR/bus" ]; then
  echo '用户 systemd 会话不可用，请由管理员启用 linger 后重新登录' >&2
  exit 1
fi
if [ ! -s /home/orderhub/systemd/orderhub.service ]; then
  echo '缺少发布包中的 systemd/orderhub.service，请补齐后继续' >&2
  exit 1
fi
mkdir -p /home/orderhub/systemd/orderhub.service.d
mkdir -p /home/orderhub/.config/systemd/user
for name in orderhub.service orderhub.service.d; do
  target="/home/orderhub/systemd/$name"
  link="/home/orderhub/.config/systemd/user/$name"
  if [ -L "$link" ] && [ "$(readlink "$link")" = "$target" ]; then
    continue
  fi
  if [ -e "$link" ] || [ -L "$link" ]; then
    printf '已有注册项，先按第 6 节保留并迁移：%s\n' "$link" >&2
    exit 1
  fi
done
for name in orderhub.service orderhub.service.d; do
  link="/home/orderhub/.config/systemd/user/$name"
  if [ ! -L "$link" ]; then
    ln -s "/home/orderhub/systemd/$name" "$link"
  fi
done
systemctl --user daemon-reload
systemctl --user show orderhub.service --no-pager -p EnvironmentFiles -p DropInPaths -p WorkingDirectory -p ExecStart
systemctl --user start orderhub.service
systemctl --user status orderhub.service --no-pager -l
journalctl --user -u orderhub.service -n 50 --no-pager
```

确认模拟交易所、输入生产者的启动顺序后，再执行 `systemctl --user enable orderhub.service`。

需要停止时单独执行：

```bash
systemctl --user stop orderhub.service
systemctl --user show orderhub.service --no-pager -p Result -p ExecMainStatus
```

正常值为 `success / 0`，节点日志应有 `stopped; checkpoint=...`。
unit 使用 SIGINT，停止超时 60 秒；`Restart=no`。异常退出按恢复流程处理。

## 5. 检查和业务输入

安全组和防火墙按客户端来源允许 TCP 50051。启动后检查监听和 FIX 日志：

```bash
ss -lntp '( sport = :50051 )'
fix_log=/home/orderhub/logs/fix/FIX.4.2-ORDERHUB-GOX.event.current.log
if [ -f "$fix_log" ]; then
  tail -n 20 "$fix_log"
else
  echo 'FIX 事件日志尚未生成，请先查看第 4 节的服务日志和 FIX 配置'
fi
```

日志文件名中的会话标识应与实际 FIX 配置一致。FIX 日志应有登录成功和双向心跳；`messages.current.log`
记录收发报文，`event.current.log` 记录会话事件，每类默认 10 MiB 加 5 个历史文件。
TLS 保护 gRPC；FIX 为普通 TCP，跨机保护由部署网络提供。

服务运行、TLS 及 FIX 正常不等于业务可下单。默认 `ORDERHUB_INPUT_STDIN=0`，
当前仍需合约、合法报价、账户和交易状态输入，再以只读 GetState 确认 `writable=true`。
包内 `orderhub-client` 会自动发单，不用于健康检查。

仅在已有持续 JSONL 生产者时，正常停机后设置 `ORDERHUB_INPUT_STDIN=1`。
先准备 FIFO，避免访问尚不存在的管道：

```bash
set -euo pipefail
mkdir -p /home/orderhub/data /home/orderhub/systemd/orderhub.service.d
fifo=/home/orderhub/data/input.fifo
if [ ! -e "$fifo" ] && [ ! -L "$fifo" ]; then
  mkfifo -m 600 "$fifo"
elif [ ! -p "$fifo" ] || [ -L "$fifo" ]; then
  echo 'input.fifo 已存在但不是普通 FIFO，请核对后继续' >&2
  exit 1
fi
```

在已创建的 `systemd/orderhub.service.d/` 中新建 `input.conf`；若文件已有内容，先读取并保留后编辑：

```ini
[Service]
StandardInput=file:%h/data/input.fifo
```

保存后执行 `systemctl --user daemon-reload`。以 `orderhub` 用户把生产者输出送到 FIFO，
再启动节点；生产者必须持续运行。未接入生产者时跳过本项，保持 `ORDERHUB_INPUT_STDIN=0`。
首次 FIFO 两端连接前可能等待。EOF 会关闭输入门禁，不循环回放过期账户/状态。
这仍是现有本地 fixture 入口，独立数据 API 尚未实现；FIX 适配器不负责账户查询。

## 6. 已有实例更新

更新与首次部署要先区分，不能仅因 env 文件存在就判断为旧实例：运行包本身已经包含 env。

- **人工解压前**：正常停止已注册服务，确认 `success / 0`。读取实际存在的旧 env，
  确定原业务库路径；只在该库存在时用旧版 `orderhub-recovery backup` 备份。
  若原库按部署记录应存在但已缺失，暂停更新，不创建空库。备份现有配置、程序、unit 和 drop-in。
- **人工解压时**：保留已有 `config/`、`logs/` 和业务数据，只更新程序、unit 和发布说明。
  若已经覆盖配置，应先从备份恢复，确认后再交给 agent。源码包不参与部署。
- **agent 接手后**：从第 2 节检查解压结果，复用旧 TLS、Token、账户权限和 FIX 配置。
  保持 `ORDERHUB_JOURNAL` 和 `ORDERHUB_CHECKPOINT_DIR` 指向原数据位置。
  不搜索或读取假定存在的旧目录；仅检查旧 env 实际指定的路径。
- **旧注册项处理**：仅在存在时读取旧链接、unit 和 drop-in；先创建备份目录并备份已有项。
  将实际存在的旧 drop-in 合并到 `systemd/orderhub.service.d/`，同名文件冲突时先核对内容。
  把标准注册位置的旧文件或链接改名留存后，再执行第 4 节创建新链接。
  若旧链接目标已经不存在，保留链接本身，不尝试读取其目标。

配置和业务库路径确认后才启动。若要把原数据迁到 `data/`，必须停机后完整迁移并修改路径，
不能把路径改到空目录来创建新库。备份目录必须先创建，业务库只有首次启动成功后才可能生成。

有历史订单、备份恢复或异常 async 会话时，重启可能保持 HALTED，需要外部对账；
不能删库解除门禁。async ACK 可能先于刷盘；报价仅在内存，重启后需要重新输入。
Redis/PostgreSQL 未启用。

GitHub 冒烟覆盖 CentOS 9 容器用户态的解包、TLS、SIGINT、备份恢复及 unit 语法，
未验证完整主机启动、真实用户 systemd 会话、SELinux、真实 FIX 场所或断电场景。
