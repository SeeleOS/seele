#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")/.."

runner_log="/tmp/seele-agent.log"
timeout_seconds="${SEELE_USERSPACE_BOOT_TIMEOUT:-180}"
userspace_pattern='Welcome to Arch Linux|Reached target|login|systemd'

: > "$runner_log"

agent-tools/run-agent-vm.sh &
runner_pid=$!

cleanup() {
    if kill -0 "$runner_pid" 2>/dev/null; then
        kill "$runner_pid" 2>/dev/null || true
        sleep 1
    fi

    for pattern in qemu-system-x86_64 seeleos-runner run-agent-vm.sh; do
        while read -r pid _; do
            [ -n "$pid" ] || continue
            [ "$pid" = "$$" ] && continue
            kill "$pid" 2>/dev/null || true
        done < <(ps -eo pid=,comm= | awk -v pattern="$pattern" '$2 ~ pattern { print $1, $2 }')
    done
}

trap cleanup EXIT INT TERM

deadline=$((SECONDS + timeout_seconds))
while [ "$SECONDS" -lt "$deadline" ]; do
    if grep -Eiq "$userspace_pattern" "$runner_log"; then
        echo "userspace boot test: startup signal observed"
        cleanup
        trap - EXIT INT TERM
        break
    fi

    if ! kill -0 "$runner_pid" 2>/dev/null; then
        wait "$runner_pid" || true
        echo "userspace boot test: VM wrapper exited before startup signal" >&2
        exit 1
    fi

    sleep 1
done

if [ "$SECONDS" -ge "$deadline" ]; then
    echo "userspace boot test: timed out waiting for startup signal" >&2
    exit 1
fi

leftovers="$(ps -eo comm= | grep -E '^(qemu-system-x86_64|seeleos-runner|run-agent-vm.sh)$' || true)"
if [ -n "$leftovers" ]; then
    echo "userspace boot test: leftover VM processes detected:" >&2
    echo "$leftovers" >&2
    exit 1
fi
