#!/bin/sh

set -eu

cd "$(dirname "$0")/.."

log_file="${LOG_FILE:-/tmp/seele-graphical.log}"
mkdir -p "$(dirname "$log_file")"

nix develop -c cargo run 2>&1 | tee "$log_file"
