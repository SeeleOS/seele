#!/bin/sh

set -eu

cd "$(dirname "$0")/.."

log_file="${LOG_FILE:-/tmp/seele-agent.log}"
export SEELE_QEMU_TIMEOUT="${SEELE_QEMU_TIMEOUT:-45s}"
mkdir -p "$(dirname "$log_file")"

nix develop -c cargo run -- --agent 2>&1 | tee "$log_file"
