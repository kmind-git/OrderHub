#!/usr/bin/env bash
set -euo pipefail

# Linux-only: packages ELF binaries built in the CentOS Stream 9 workflow
main() {
  if [[ $# != 3 || ! "$3" =~ ^[0-9a-f]{40}$ ]]; then
    echo 'Usage: package-centos9.bash BINARY_DIRECTORY OUTPUT_DIRECTORY COMMIT_SHA' >&2
    exit 2
  fi
  local binaries output commit root name package binary
  binaries="$(cd "$1" && pwd)"
  output="$2"
  commit="$3"
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  [[ ! -e "$output" ]] || {
    echo "Output already exists: $output" >&2
    exit 1
  }
  [[ "$(git -C "$root" rev-parse HEAD)" == "$commit" ]] || {
    echo 'Commit does not match the checked out source' >&2
    exit 1
  }
  for binary in orderhub-node orderhub-client orderhub-recovery; do
    [[ -x "$binaries/$binary" ]] || {
      echo "Missing executable: $binary" >&2
      exit 1
    }
  done
  mkdir -p "$output"
  output="$(cd "$output" && pwd)"
  name="orderhub-centos9-x86_64-${commit:0:12}"
  package="$output/$name"
  mkdir -p "$package/bin" "$package/deploy" "$package/docs" "$package/verification"
  for binary in orderhub-node orderhub-client orderhub-recovery; do
    install -m 0755 "$binaries/$binary" "$package/bin/$binary"
    file "$package/bin/$binary" > "$package/verification/$binary.file.txt"
    readelf --version-info "$package/bin/$binary" > "$package/verification/$binary.versions.txt"
    ldd "$package/bin/$binary" > "$package/verification/$binary.ldd.txt"
    if grep -q 'not found' "$package/verification/$binary.ldd.txt"; then
      echo "Unresolved runtime dependency: $binary" >&2
      exit 1
    fi
    if ! grep -q 'ELF 64-bit.*x86-64' "$package/verification/$binary.file.txt"; then
      echo "Wrong binary target: $binary" >&2
      exit 1
    fi
    if grep -oE 'GLIBC_[0-9]+\.[0-9]+' "$package/verification/$binary.versions.txt" |
      sort -Vu | awk -F '[_.]' '$2 > 2 || ($2 == 2 && $3 > 34) { bad = 1 } END { exit !bad }'; then
      echo "Binary requires glibc newer than 2.34: $binary" >&2
      exit 1
    fi
  done
  install -m 0644 "$root/deploy/centos9/orderhub.service" "$package/deploy/"
  install -m 0644 "$root/deploy/centos9/orderhub.env.example" "$package/deploy/"
  for binary in centos9-deployment persistence-policy recovery-runtime tls input-runtime; do
    install -m 0644 "$root/docs/orderhub/$binary.md" "$package/docs/"
  done
  install -m 0644 "$root/Cargo.lock" "$package/"
  git -C "$root" archive --format=tar.gz --prefix=OrderHub-source/ \
    --output="$package/source.tar.gz" "$commit"
  {
    printf 'repository=https://github.com/kmind-git/OrderHub\ncommit=%s\n' "$commit"
    printf 'target=x86_64-unknown-linux-gnu\nfeatures=grpc-tls\nprofile=release\n'
    printf 'build_os=CentOS Stream 9\n'
    rustc --version
    cargo --version
    rpm -q glibc
    printf 'rustflags=%s\n' "${RUSTFLAGS:-repository defaults}"
  } > "$package/BUILD-INFO.txt"
  cat > "$package/SOURCE-NOTICES.txt" << 'NOTICES'
This package includes source.tar.gz for the exact OrderHub commit in BUILD-INFO.txt.
Preserve source copyright notices, crate license declarations and patch provenance.
Third-party dependencies are identified by Cargo.lock and retain their original terms.
NOTICES
  (cd "$package" && find . -type f -print0 |
    sort -z | xargs -0 sha256sum > "$output/$name.SHA256SUMS")
  mv "$output/$name.SHA256SUMS" "$package/SHA256SUMS"
  tar -czf "$output/$name.tar.gz" -C "$output" "$name"
  (cd "$output" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
}

main "$@"
