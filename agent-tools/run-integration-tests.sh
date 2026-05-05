#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")/.."

log_file="${LOG_FILE:-/tmp/seele-integration-tests.log}"
mkdir -p "$(dirname "$log_file")"

SEELE_QEMU_TIMEOUT="${SEELE_QEMU_TIMEOUT:-60s}" nix develop -c cargo run --bin seeleos-runner-integration-test 2>&1 | tee "$log_file"
