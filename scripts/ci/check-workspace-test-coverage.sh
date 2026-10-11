#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/../.."
exec cargo run --locked -p orderhub-tools --no-default-features --features checks --bin orderhub-check -- workspace-tests
