#!/usr/bin/env bash
set -euo pipefail

# Linux-only: runs the extracted TLS package as the non-root deployment user
main() {
  [[ $# == 2 ]] || {
    echo 'Usage: smoke-centos9.bash PACKAGE_DIRECTORY VERIFICATION_DIRECTORY' >&2
    exit 2
  }
  [[ "$(id -un)" == orderhub && "$(id -u)" != 0 ]] || {
    echo 'Smoke test must run as the non-root orderhub user' >&2
    exit 1
  }
  local package attempt client_status
  package="$(cd "$1" && pwd)"
  mkdir -p "$2"
  verification="$(cd "$2" && pwd)"
  (cd "$package" && sha256sum -c SHA256SUMS)
  work="$(mktemp -d "$verification/smoke.XXXXXXXX")"
  trap cleanup EXIT
  openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=localhost \
    -addext subjectAltName=DNS:localhost -addext basicConstraints=critical,CA:FALSE \
    -addext keyUsage=critical,digitalSignature,keyEncipherment -addext extendedKeyUsage=serverAuth \
    -keyout "$work/key.pem" -out "$work/cert.pem" \
    > "$work/certificate.log" 2>&1
  sed -e 's/^SocketConnectPort=.*/SocketConnectPort=1/' \
    -e 's|^FileLogPath=.*|FileLogPath=./fix-logs|' \
    "$package/config/orderhub-fix.cfg" > "$work/orderhub-fix.cfg"
  export ORDERHUB_ADDR=127.0.0.1:50051 ORDERHUB_FIX_CONFIG="$work/orderhub-fix.cfg"
  export ORDERHUB_JOURNAL="$work/journal.redb" ORDERHUB_CHECKPOINT_DIR="$work/checkpoints"
  export ORDERHUB_TOKEN=isolated-release-smoke ORDERHUB_PERSISTENCE=async ORDERHUB_INPUT_STDIN=0
  export ORDERHUB_TLS_CERT="$work/cert.pem" ORDERHUB_TLS_KEY="$work/key.pem"
  export ORDERHUB_TLS_CA="$work/cert.pem" ORDERHUB_TLS_DOMAIN=localhost
  unset ORDERHUB_SUBMITTERS ORDERHUB_REDIS_ENABLED ORDERHUB_POSTGRES_ENABLED ORDERHUB_EXCHANGE_ADDR
  "$package/bin/orderhub-node" > "$work/node.log" 2>&1 &
  node_pid=$!
  for ((attempt = 0; attempt < 100; attempt++)); do
    kill -0 "$node_pid"
    if grep -q 'serving tls/' "$work/node.log"; then break; fi
    sleep 0.1
  done
  grep -q 'serving tls/' "$work/node.log"
  client_status=0
  "$package/bin/orderhub-client" > "$work/client.log" 2>&1 || client_status=$?
  [[ "$client_status" != 0 ]]
  grep -q 'OrderHub new-order gate is closed' "$work/client.log"
  kill -INT "$node_pid"
  for ((attempt = 0; attempt < 600; attempt++)); do
    if ! kill -0 "$node_pid" 2> /dev/null; then break; fi
    sleep 0.1
  done
  if kill -0 "$node_pid" 2> /dev/null; then
    echo 'Node did not complete graceful shutdown within 60 seconds' >&2
    exit 1
  fi
  wait "$node_pid"
  node_pid=''
  grep -q 'stopped; checkpoint=' "$work/node.log"
  grep -q 'CONNECT_FAILED' "$work/fix-logs/FIX.4.2-ORDERHUB-GOX.event.current.log"
  grep -q 'NODE_STOPPED core shutdown completed' "$work/fix-logs/FIX.4.2-ORDERHUB-GOX.event.current.log"
  "$package/bin/orderhub-recovery" inspect "$work/journal.redb" > "$work/recovery.json"
  grep -q '"unclean_async_restart": false' "$work/recovery.json"
  "$package/bin/orderhub-recovery" backup "$work/journal.redb" "$work/backup" \
    > "$work/manifest-path.txt"
  "$package/bin/orderhub-recovery" restore "$(cat "$work/manifest-path.txt")" "$work/restored.redb" \
    > "$work/restored.json"
  grep -q '"backup_restore_fenced": true' "$work/restored.json"
  # Only diagnostic logs leave the job; certificates, private keys and databases are ephemeral
  cp "$work/node.log" "$work/client.log" "$work/recovery.json" "$work/restored.json" \
    "$verification/"
  cp "$work/fix-logs/FIX.4.2-ORDERHUB-GOX.event.current.log" "$verification/"
  printf 'user=orderhub\nTLS GetState=passed\nSIGINT shutdown=passed\nbackup restore=passed\n' \
    > "$verification/smoke-result.txt"
}

cleanup() {
  local status=$? log
  if [[ "$status" != 0 && -n "${work:-}" ]]; then
    echo "CentOS smoke failed with status $status" >&2
    for log in certificate node client recovery restored; do
      if [[ -f "$work/$log.log" ]]; then
        cat "$work/$log.log" >&2
        cp "$work/$log.log" "$(dirname "$work")/failed-$log.log"
      fi
    done
  fi
  if [[ -n "${node_pid:-}" ]] && kill -0 "$node_pid" 2> /dev/null; then
    kill -KILL "$node_pid" 2> /dev/null || true
    wait "$node_pid" 2> /dev/null || true
  fi
  if [[ -n "${work:-}" && -d "$work" ]]; then rm -rf -- "$work"; fi
}

main "$@"
