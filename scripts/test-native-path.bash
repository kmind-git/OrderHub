#!/usr/bin/env bash
set -euo pipefail

main() {
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
  # shellcheck source=scripts/native-path.bash
  source "$script_dir/native-path.bash"
  case_root="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/nautilus-path.XXXXXX")"
  trap 'rm -rf "$case_root"' EXIT

  test_platforms
  test_errors
  test_native_tools
  echo 'Native path tests passed'
}

test_platforms() (
  local mock_platform path actual
  uname() { printf '%s\n' "$mock_platform"; }
  for mock_platform in Linux Darwin; do
    for path in '/tmp/path with spaces' '../wheel dir/package.whl' 'C:/already native'; do
      actual="$(native_path "$path")"
      [[ "$actual" == "$path" ]] || return 1
    done
  done

  cygpath() {
    [[ "$#" -eq 3 && "$1" == -m && "$2" == -- && "$3" == "$path" ]] || return 2
    printf '%s\n' 'C:/converted path'
  }
  for mock_platform in MINGW64_NT-10.0 MSYS_NT-10.0 CYGWIN_NT-10.0; do
    for path in '/c/wheel dir' '/cygdrive/c/wheel dir' '../wheel dir' 'C:\wheel dir' 'C:/wheel dir' '//server/share/wheel dir'; do
      actual="$(native_path "$path")"
      [[ "$actual" == 'C:/converted path' ]] || return 1
    done
  done
)

test_errors() (
  local status uname_status=0
  if native_path > "$case_root/output" 2>&1 ||
    native_path '' > "$case_root/output" 2>&1 ||
    native_path one two > "$case_root/output" 2>&1; then
    echo 'Invalid path arguments were accepted' >&2
    return 1
  fi

  uname() {
    echo MINGW64_NT-10.0
    return "$uname_status"
  }
  mkdir "$case_root/empty-bin"
  status=0
  PATH="$case_root/empty-bin" native_path /c/wheel > "$case_root/output" 2>&1 || status=$?
  [[ "$status" -eq 1 ]] || return 1
  grep -Fq 'require cygpath' "$case_root/output"

  cygpath() { return 37; }
  status=0
  native_path /c/wheel > "$case_root/output" 2>&1 || status=$?
  [[ "$status" -eq 37 ]] || return 1

  uname_status=38
  status=0
  native_path /c/wheel > "$case_root/output" 2>&1 || status=$?
  [[ "$status" -eq 38 ]] || return 1
)

test_native_tools() (
  local source_dir source_path value
  mkdir "$case_root/path with spaces"
  cd "$case_root"
  printf '%s\n' 'VALUE = 37' > 'path with spaces/probe.txt'
  source_dir="$(native_path "$case_root/path with spaces")"
  source_path="$(native_path "$case_root/path with spaces/probe.txt")"
  value=$(jq -nr --rawfile value "$source_path" '$value | rtrimstr("\n")')
  [[ "$value" == 'VALUE = 37' ]]
  value=$(jq -nr --rawfile value "$(native_path 'path with spaces/probe.txt')" '$value | rtrimstr("\n")')
  [[ "$value" == 'VALUE = 37' ]]

  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
      [[ "$(native_path "$source_dir")" == "$source_dir" ]]
      [[ "$(native_path "${source_dir//\//\\}")" == "$source_dir" ]]
      ;;
  esac
)

main "$@"
