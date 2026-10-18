#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work_root="$(cd "${TMPDIR:-/tmp}" && pwd)"
work="$(mktemp -d "$work_root/orderhub-package.XXXXXXXX")"
cleanup() {
  [[ "$work" == "$work_root"/orderhub-package.* && -d "$work" && ! -L "$work" ]] || return 1
  rm -rf -- "$work"
}
trap cleanup EXIT
mkdir -p "$work/tools" "$work/bin"
export PATH="$work/tools:$PATH"
commit=1111111111111111111111111111111111111111
for name in orderhub-node orderhub-client orderhub-recovery; do
  printf '#!/usr/bin/env bash\nexit 0\n' > "$work/bin/$name"
  chmod +x "$work/bin/$name"
done
cat > "$work/tools/git" << 'MOCK'
#!/usr/bin/env bash
set -eu
for value in "$@"; do
  if [[ "$value" == rev-parse ]]; then
    echo 1111111111111111111111111111111111111111
    exit 0
  fi
done
for value in "$@"; do
  if [[ "$value" == --output=* ]]; then printf source > "${value#--output=}"; fi
done
MOCK
cat > "$work/tools/file" << 'MOCK'
#!/usr/bin/env bash
echo 'ELF 64-bit LSB pie executable, x86-64, dynamically linked'
MOCK
cat > "$work/tools/readelf" << 'MOCK'
#!/usr/bin/env bash
echo "Name: GLIBC_${TEST_GLIBC_VERSION:-2.34}"
MOCK
cat > "$work/tools/ldd" << 'MOCK'
#!/usr/bin/env bash
echo "${TEST_LDD_RESULT:-libc.so.6 => /lib64/libc.so.6}"
MOCK
for name in rustc cargo rpm; do
  printf '#!/usr/bin/env bash\necho mock-version\n' > "$work/tools/$name"
done
chmod +x "$work/tools/"*

expect_failure() {
  local expected="$1"
  shift
  if bash "$root/scripts/orderhub/package-centos9.bash" "$@" > "$work/error" 2>&1; then
    echo 'Expected packaging to fail' >&2
    exit 1
  fi
  grep -q "$expected" "$work/error"
}

expect_failure Usage
expect_failure 'Commit does not match' "$work/bin" "$work/wrong" \
  2222222222222222222222222222222222222222
mkdir "$work/existing"
expect_failure 'Output already exists' "$work/bin" "$work/existing" "$commit"
mv "$work/bin/orderhub-client" "$work/client"
expect_failure 'Missing executable' "$work/bin" "$work/missing" "$commit"
mv "$work/client" "$work/bin/orderhub-client"
export TEST_GLIBC_VERSION=2.35
expect_failure 'newer than 2.34' "$work/bin" "$work/glibc" "$commit"
unset TEST_GLIBC_VERSION
export TEST_LDD_RESULT='libexample.so => not found'
expect_failure 'Unresolved runtime dependency' "$work/bin" "$work/ldd" "$commit"
unset TEST_LDD_RESULT
bash "$root/scripts/orderhub/package-centos9.bash" "$work/bin" "$work/success" "$commit"
name=orderhub-centos9-x86_64-111111111111
(cd "$work/success" && sha256sum -c "$name.tar.gz.sha256" &&
  sha256sum -c "$name-source.tar.gz.sha256")
mkdir -p "$work/extracted/config/tls"
printf 'existing-private-key\n' > "$work/extracted/config/tls/server-key.pem"
tar --no-same-owner --keep-old-files \
  -xzf "$work/success/$name.tar.gz" -C "$work/extracted"
(cd "$work/extracted" && sha256sum -c SHA256SUMS)
grep -q "commit=$commit" "$work/extracted/BUILD-INFO.txt"
[[ "$(find "$work/extracted" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort | paste -sd ,)" == bin,config,logs,systemd ]]
[[ -d "$work/extracted/config/tls" && -d "$work/extracted/logs/fix" ]]
[[ -d "$work/extracted/systemd/orderhub.service.d" ]]
grep -q '^existing-private-key$' "$work/extracted/config/tls/server-key.pem"
[[ -f "$work/extracted/deployment.md" && -f "$work/extracted/systemd/orderhub.service" ]]
grep -q '^WorkingDirectory=%h$' "$work/extracted/systemd/orderhub.service"
grep -q '^EnvironmentFile=%h/config/orderhub.env$' "$work/extracted/systemd/orderhub.service"
grep -q '^ExecStart=%h/bin/orderhub-node$' "$work/extracted/systemd/orderhub.service"
grep -q '^ExecStartPre=/usr/bin/mkdir -p %h/data/checkpoints$' "$work/extracted/systemd/orderhub.service"
grep -q '^ORDERHUB_JOURNAL=/home/orderhub/data/orderhub-journal.redb$' "$work/extracted/config/orderhub.env"
grep -q '^ORDERHUB_FIX_CONFIG=/home/orderhub/config/orderhub-fix.cfg$' "$work/extracted/config/orderhub.env"
grep -q '^ORDERHUB_TLS_KEY=/home/orderhub/config/tls/server-key.pem$' "$work/extracted/config/orderhub.env"
grep -q '^ORDERHUB_TEST_ACCOUNT=0$' "$work/extracted/config/orderhub.env"
grep -q 'SenderCompID=ORDERHUB' "$work/extracted/config/orderhub-fix.cfg"
[[ -f "$work/success/$name-source.tar.gz" && -f "$work/success/verification/orderhub-node.ldd.txt" ]]
grep -q "$name-source.tar.gz" "$work/extracted/SOURCE-NOTICES.txt"
[[ -z "$(tar -tzf "$work/success/$name.tar.gz" | grep -E '\.(pem|redb)$' || true)" ]]

# Updating binaries and units must preserve instance configuration, TLS and logs
printf 'instance-config\n' > "$work/extracted/config/orderhub.env"
printf 'existing-fix-log\n' > "$work/extracted/logs/fix/session.log"
tar --no-same-owner --no-overwrite-dir --exclude=config --exclude=logs \
  -xzf "$work/success/$name.tar.gz" -C "$work/extracted"
grep -q '^instance-config$' "$work/extracted/config/orderhub.env"
grep -q '^existing-private-key$' "$work/extracted/config/tls/server-key.pem"
grep -q '^existing-fix-log$' "$work/extracted/logs/fix/session.log"
(cd "$work/extracted" && grep -E ' [ *]\./bin/' SHA256SUMS | sha256sum -c -)

echo 'CentOS packaging: final layout, preserved instance files and six failure paths passed'
