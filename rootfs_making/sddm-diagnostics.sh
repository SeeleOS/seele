#!/bin/sh

set -eu

log=/var/log/sddm/diagnostics.log

append() {
    {
        echo "=== $1 ==="
        shift
        "$@" || true
        echo
    } >>"${log}" 2>&1
    sync
}

rm -f "${log}"
append date date
sleep 10
append display-manager-link ls -l /etc/systemd/system/display-manager.service
append proc-processes sh -c 'for proc in /proc/[0-9]*; do pid=${proc#/proc/}; comm=$(cat "${proc}/comm" 2>/dev/null || true); case "${comm}" in sddm|kwin*|plasmashell|startplasma*|Xorg|Xephyr|dbus*|systemd-logind|seatd) echo "${pid} ${comm}";; esac; done'
append sddm-directories ls -la /run/sddm /var/lib/sddm /var/log/sddm
append run-user-root ls -la /run/user/0
append devices ls -la /dev/dri /dev/input /dev/tty0 /dev/tty1 /dev/fb0
append sddm-config sh -c 'for file in /etc/sddm.conf /etc/sddm.conf.d/*.conf; do [ -e "${file}" ] && echo "--- ${file}" && sed -n "1,160p" "${file}"; done'
