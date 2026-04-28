#!/bin/sh

set -eu

log=/var/log/sddm/diagnostics.log
tmp="${log}.tmp"

run_probe() {
    echo "=== $* ==="
    timeout 5s "$@" || echo "probe failed or timed out: $*"
}

{
    date
    run_probe systemctl status --no-pager sddm.service
    run_probe systemctl show --no-pager sddm.service
    echo '=== display-manager link ==='
    ls -l /etc/systemd/system/display-manager.service || true
    run_probe ps -ef
    echo '=== sddm directories ==='
    ls -la /run/sddm /var/lib/sddm /var/log/sddm 2>&1 || true
    run_probe loginctl seat-status seat0
    run_probe loginctl list-sessions
    run_probe journalctl --no-pager -n 120
} >"${tmp}" 2>&1

mv "${tmp}" "${log}"
