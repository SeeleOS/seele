#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")/.."

log_file="${LOG_FILE:-/tmp/seele-tests.log}"
mkdir -p "$(dirname "$log_file")"

nix develop -c cargo run --bin seeleos-runner-test 2>&1 | tee "$log_file"
