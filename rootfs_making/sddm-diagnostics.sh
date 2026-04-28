#!/bin/sh

set -eu

log=/var/log/sddm/diagnostics.log
tmp="${log}.tmp"

{
    date
    echo '=== systemctl status sddm ==='
    systemctl status --no-pager sddm.service || true
    echo '=== systemctl show sddm ==='
    systemctl show --no-pager sddm.service || true
    echo '=== display-manager link ==='
    ls -l /etc/systemd/system/display-manager.service || true
    echo '=== processes ==='
    ps -ef || true
    echo '=== sddm directories ==='
    ls -la /run/sddm /var/lib/sddm /var/log/sddm 2>&1 || true
    echo '=== seats ==='
    loginctl seat-status seat0 || true
    echo '=== sessions ==='
    loginctl list-sessions || true
    echo '=== journal tail ==='
    journalctl --no-pager -n 120 || true
} >"${tmp}" 2>&1

mv "${tmp}" "${log}"
