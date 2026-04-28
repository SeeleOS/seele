#!/bin/sh

set -u

log=/var/log/sddm/diagnostics.log
console=/dev/console

emit() {
    printf "%s\n" "$*" >>"${log}"
    printf "%s\n" "$*" >"${console}" 2>/dev/null || true
}

section() {
    emit "=== $1 ==="
}

rm -f "${log}"

section date
date >>"${log}" 2>&1 || true

sleep 10

section proc-processes
for proc in /proc/[0-9]*; do
    pid=${proc#/proc/}
    comm=$(cat "${proc}/comm" 2>/dev/null || true)
    case "${comm}" in
        sddm|kwin*|plasmashell|startplasma*|Xorg|Xephyr|dbus*|systemd-logind|seatd)
            emit "${pid} ${comm}"
            cat "${proc}/status" >>"${log}" 2>/dev/null || true
            cat "${proc}/wchan" >>"${log}" 2>/dev/null || true
            emit ""
            ;;
    esac
done

section sddm-runtime-files
for path in /run/sddm /run/sddm/* /var/lib/sddm /var/lib/sddm/* /var/log/sddm /var/log/sddm/* /run/user/0 /run/user/0/*; do
    emit "${path}"
done

section devices
for path in /dev/dri /dev/dri/* /dev/input /dev/input/* /dev/tty0 /dev/tty1 /dev/fb0; do
    emit "${path}"
done


section logind-runtime
for path in /run/systemd/seats /run/systemd/seats/* /run/systemd/sessions /run/systemd/sessions/* /run/udev/tags/seat /run/udev/tags/seat/* /run/udev/tags/master-of-seat /run/udev/tags/master-of-seat/*; do
    emit "${path}"
done
for file in /run/systemd/seats/seat0 /run/udev/data/+drm:card0 /run/udev/data/c13:64 /run/udev/data/c13:65; do
    emit "--- ${file}"
    cat "${file}" 2>&1 | while IFS= read -r line; do
        emit "${line}"
    done
done

section sddm-config
for file in /etc/sddm.conf /etc/sddm.conf.d/seele-wayland.conf; do
    emit "--- ${file}"
    cat "${file}" >>"${log}" 2>&1 || true
done

sleep 60

section user-runtime-dir
for path in /run/user/961 /run/user/961/*; do
    emit "${path}"
done

section user-manager-status
systemctl --no-pager --full status user-runtime-dir@961.service user@961.service >>"${log}" 2>&1 || true
systemctl --no-pager --full show user-runtime-dir@961.service user@961.service >>"${log}" 2>&1 || true
journalctl --no-pager -u user-runtime-dir@961.service -u user@961.service -n 200 >>"${log}" 2>&1 || true

sync
