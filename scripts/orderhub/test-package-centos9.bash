#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
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
(cd "$work/success" && sha256sum -c "$name.tar.gz.sha256")
mkdir "$work/extracted"
tar -xzf "$work/success/$name.tar.gz" -C "$work/extracted"
(cd "$work/extracted/$name" && sha256sum -c SHA256SUMS)
grep -q "commit=$commit" "$work/extracted/$name/BUILD-INFO.txt"
[[ -f "$work/extracted/$name/deploy/orderhub.service" ]]
[[ -f "$work/extracted/$name/source.tar.gz" ]]
echo 'CentOS packaging: success and six failure paths passed'
