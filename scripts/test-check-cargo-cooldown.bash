#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

test_root="$(mktemp -d "${TMPDIR:-/tmp}/nautilus-cooldown-consumer.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

git -C "$REPO_ROOT" ls-files '*Cargo.lock' > "${test_root}/actual-locks"
printf '%s\n' \
  'Cargo.lock' \
  > "${test_root}/expected-locks"

if ! cmp -s "${test_root}/expected-locks" "${test_root}/actual-locks"; then
  echo "Tracked Cargo lockfiles do not match the OrderHub consumer layout" >&2
  diff -u "${test_root}/expected-locks" "${test_root}/actual-locks" >&2 || true
  exit 1
fi

fixture_repo="${test_root}/repo"
fake_bin="${test_root}/bin"
fuzz_path="crates/adapters/lighter/fuzz/pornin"
mkdir -p "${fixture_repo}/scripts" "${fixture_repo}/${fuzz_path}" "$fake_bin"
cp "${COOLDOWN_SCRIPT_SOURCE:-${REPO_ROOT}/scripts/check-cargo-cooldown.sh}" "${fixture_repo}/scripts/"

printf '%s\n' \
  '[workspace]' \
  'members = []' \
  '' \
  '[workspace.metadata.cooldown]' \
  'days = 3' \
  > "${fixture_repo}/Cargo.toml"
printf '%s\n' '[workspace]' 'members = []' > "${fixture_repo}/${fuzz_path}/Cargo.toml"
printf '%s\n' 'version = 4' > "${fixture_repo}/Cargo.lock"
cp "${fixture_repo}/Cargo.lock" "${fixture_repo}/${fuzz_path}/Cargo.lock"

cat > "${fake_bin}/curl" << 'FAKE_CURL'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "${COOLDOWN_NETWORK_LOG:?}"
echo "Cargo cooldown consumer check unexpectedly accessed the network" >&2
exit 1
FAKE_CURL
chmod +x "${fake_bin}/curl"
export COOLDOWN_NETWORK_LOG="${test_root}/network.log"

git -C "$fixture_repo" init --quiet
git -C "$fixture_repo" config user.email test@example.com
git -C "$fixture_repo" config user.name Test
git -C "$fixture_repo" config commit.gpgsign false
git -C "$fixture_repo" add -A
git -C "$fixture_repo" commit --quiet -m baseline

if [[ ! -f "$REPO_ROOT/.supply-chain/crate-dates.json" ]]; then
  echo "Cooldown database is missing" >&2
  exit 1
fi
if git -C "$REPO_ROOT" check-ignore -q .supply-chain/crate-dates.json; then
  echo "Cooldown database is ignored" >&2
  exit 1
fi
if ! grep -Fq 'crate-dates' "$REPO_ROOT/.pre-commit-config.yaml"; then
  echo "cargo-cooldown hook does not watch the publication-date database" >&2
  exit 1
fi

command -v jq > /dev/null || { echo "jq is required" >&2; exit 1; }
awk '
  function emit() { if (name && version && source ~ /crates.io/) print name "@" version }
  /^\[\[package\]\]/ { emit(); name=version=source=""; next }
  /^name = "/ { name=$0; sub(/^name = "/,"",name); sub(/"$/,"",name) }
  /^version = "/ { version=$0; sub(/^version = "/,"",version); sub(/"$/,"",version) }
  /^source = "/ { source=$0 }
  END { emit() }
' "$REPO_ROOT/Cargo.lock" | LC_ALL=C sort -u > "${test_root}/registry-keys"
jq -r '.entries | keys[]' "$REPO_ROOT/.supply-chain/crate-dates.json" |
  awk '{ sub(/\r$/, ""); print }' |
  LC_ALL=C sort -u > "${test_root}/database-keys"
if ! cmp -s "${test_root}/registry-keys" "${test_root}/database-keys"; then
  echo "Cooldown database does not match tracked registry versions" >&2
  diff -u "${test_root}/registry-keys" "${test_root}/database-keys" >&2 || true
  exit 1
fi
db_count=$(wc -l < "${test_root}/registry-keys" | tr -d ' ')

status=0
output=$(cd "$REPO_ROOT" &&
  PATH="${fake_bin}:${PATH}" bash scripts/check-cargo-cooldown.sh --all) || status=$?
if ((status != 0)) ||
  [[ "$output" != *"Publication dates: ${db_count} from the cooldown database, 0 from crates.io"* ]]; then
  printf 'Offline full cooldown check did not use the committed database: %s\n' "$output" >&2
  exit 1
fi

output=$(cd "$fixture_repo" &&
  PATH="${fake_bin}:${PATH}" bash scripts/check-cargo-cooldown.sh --all)
if [[ "$output" != "No resolved registry crate versions"* ]]; then
  printf 'Unexpected Cargo cooldown result: %s\n' "$output" >&2
  exit 1
fi

hook_entry=$(awk '
  $0 ~ /- id: cargo-cooldown$/ { in_hook = 1; next }
  in_hook && $1 == "entry:" { sub(/^[[:space:]]*entry: /, ""); print; exit }
' "$REPO_ROOT/.pre-commit-config.yaml")
read -r -a hook_command <<< "$hook_entry"
[[ ${#hook_command[@]} -gt 0 ]] || exit 1

mkdir -p "${fixture_repo}/.supply-chain"
cat >> "${fixture_repo}/Cargo.lock" << 'LOCK'

[[package]]
name = "cooldown-fixture"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
LOCK
cat > "${fixture_repo}/.supply-chain/crate-dates.json" << 'DATABASE'
{"schema":1,"entries":{"cooldown-fixture@1.0.0":{"published":"2020-01-01T00:00:00Z","verified_at":"2020-01-01T00:00:00Z"}}}
DATABASE
git -C "$fixture_repo" add Cargo.lock .supply-chain/crate-dates.json

status=0
output=$(cd "$fixture_repo" && PATH="${fake_bin}:${PATH}" \
  "${hook_command[@]}") || status=$?
if ((status != 0)) || [[ -s "$COOLDOWN_NETWORK_LOG" ]] ||
  [[ "$output" != *"Publication dates: 1 from the cooldown database, 0 from crates.io"* ]]; then
  printf 'Cooldown hook re-fetched a date added since the base: %s\n' "$output" >&2
  exit 1
fi

jq --arg now "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  '.entries["cooldown-fixture@1.0.0"].published = $now' \
  "${fixture_repo}/.supply-chain/crate-dates.json" > "${test_root}/fresh-dates.json"
mv "${test_root}/fresh-dates.json" "${fixture_repo}/.supply-chain/crate-dates.json"

status=0
output=$(cd "$fixture_repo" && PATH="${fake_bin}:${PATH}" \
  "${hook_command[@]}") || status=$?
if ((status != 1)) || [[ -s "$COOLDOWN_NETWORK_LOG" ]] ||
  [[ "$output" != *"FAIL: 1 crate(s) within the 3-day cooldown"* ]]; then
  printf 'Cooldown hook did not reject a fresh recorded crate offline: %s\n' "$output" >&2
  exit 1
fi

if awk '
  /^[^#[:space:]][^:]*:/ && !/^check-cargo-cooldown:/ &&
    /:.*check-cargo-cooldown/ { found = 1 }
  END { exit !found }
' "$REPO_ROOT/Makefile"; then
  echo "Routine Make targets still require the full cooldown check" >&2
  exit 1
fi

for source in .github/workflows/ci.yml; do
  if grep -Fq check-cargo-cooldown "$REPO_ROOT/$source"; then
    echo "Build setup still invokes the cooldown check: $source" >&2
    exit 1
  fi
done

# Make setup does not invoke cooldown; policy checks remain explicit hooks/update operations.
if grep -E '^pre-flight:.*cooldown' "$REPO_ROOT/Makefile"; then
  echo "Pre-flight must not perform dependency update policy checks" >&2
  exit 1
fi

for source in \
  Makefile \
  scripts/clippy-changed.sh \
  scripts/doc-changed.sh \
  scripts/ci/test-postgres-bootstrap.bash \
  .github/workflows/ci.yml \
  scripts/orderhub/package-db-tool.bash; do
  if ! awk '
    BEGIN {
      build = "(build|check|test|run|clippy|doc|bench|nextest[[:space:]]+run)"
      wrapper = "(miri[[:space:]]+)?" build
      wrapper = wrapper "|llvm-cov[[:space:]]+nextest|codspeed[[:space:]]+build"
      wrapper = wrapper "|hack[[:space:]].*[[:space:]](check|doc)"
      compilation = "cargo([[:space:]]+\\+[^[:space:]]+)?[[:space:]]+(" wrapper ")"
      compilation = "(" compilation "|maturin[[:space:]]+(build|develop))[[:space:]]"
    }
    FILENAME ~ /\/Makefile$/ && $0 !~ /^\t/ { next }
    /^[[:space:]]*#/ { next }
    {
      continued = sub(/\\$/, "")
      command = command $0 " "
      if (continued) next
      if (command ~ compilation &&
          command !~ /--locked([[:space:]]|$)/) {
        print FILENAME ": unlocked compilation: " command
        failed = 1
      }
      command = ""
    }
    END { exit failed }
  ' "$REPO_ROOT/$source"; then
    exit 1
  fi
done

echo "Cargo cooldown consumer check passed"
