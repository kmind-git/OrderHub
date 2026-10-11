#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
output_dir="${1:?Usage: package-db-tool.bash OUTPUT_DIRECTORY}"
profile="${ORDERHUB_DB_PROFILE:-release}"

if [[ -e "$output_dir" ]]; then
  echo "Output path already exists: $output_dir" >&2
  exit 1
fi

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) executable="orderhub-db.exe" ;;
  *) executable="orderhub-db" ;;
esac

cargo build --locked --manifest-path "$repo_root/Cargo.toml" \
  --package orderhub-tools --bin orderhub-db --profile "$profile"

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
if [[ "$profile" == "dev" ]]; then
  profile_dir="debug"
else
  profile_dir="$profile"
fi

mkdir -p "$output_dir/bin" "$output_dir/schema/sql"
cp "$target_dir/$profile_dir/$executable" "$output_dir/bin/$executable"
for name in types.sql tables.sql functions.sql partitions.sql; do
  cp "$repo_root/schema/sql/$name" "$output_dir/schema/sql/$name"
done

"$output_dir/bin/$executable" --version
"$output_dir/bin/$executable" schema-check --schema "$output_dir/schema/sql"
