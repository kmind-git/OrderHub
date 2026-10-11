#!/usr/bin/env bash
# Returns explicitly declared non-default Rust features for a crate.
#
# Usage: scripts/crate-test-features.sh <crate-name>
# Example: scripts/crate-test-features.sh nautilus-common
#   -> defi,high-precision,live,simulation,tracing-bridge

set -euo pipefail

if [ $# -ne 1 ]; then
  echo "Usage: $0 <crate-name>" >&2
  exit 1
fi

cargo metadata --locked --no-deps --format-version 1 |
  jq -r --arg p "$1" '
        [.packages[]
         | select(.name == $p)
         | .features
         | keys[]
         | select(. != "default")
        ] | join(",")'
