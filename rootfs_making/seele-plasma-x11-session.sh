#!/bin/sh

set -eu

log_file=/var/log/autoplasma.log

log() {
    line="seele-plasma-x11-session: $*"
    printf '%s\n' "${line}" >>"${log_file}"
    printf '%s\n' "${line}" >/dev/ttyS0 2>/dev/null || true
    sync
}

log "enter pid=$$ display=${DISPLAY:-unset}"

if [ -z "${DISPLAY:-}" ]; then
    log "missing DISPLAY"
    echo "seele-plasma-x11-session: DISPLAY is not set" >&2
    exit 1
fi

if [ -z "${SEELE_PLASMA_DBUS_STARTED:-}" ]; then
    log "exec dbus-run-session"
    exec /usr/bin/dbus-run-session -- env SEELE_PLASMA_DBUS_STARTED=1 "$0"
fi

export XDG_SESSION_TYPE=x11
export XDG_SESSION_DESKTOP=KDE
export XDG_CURRENT_DESKTOP=KDE
export DESKTOP_SESSION=plasma
export KDE_FULL_SESSION=true
export KDE_SESSION_VERSION=6
export QT_QPA_PLATFORM=xcb
export QT_XCB_NO_MITSHM=1
export KWIN_COMPOSE=N

unset WAYLAND_DISPLAY

log "starting kwin_x11 display=${DISPLAY:-unset} bus=${DBUS_SESSION_BUS_ADDRESS:-unset}"
/usr/bin/kwin_x11 --replace >>"${log_file}" 2>&1 &
kwin_pid=$!

sleep 2

log "starting plasmashell"
/usr/bin/plasmashell --no-respawn >>"${log_file}" 2>&1 &
shell_pid=$!

wait "${shell_pid}"
shell_status=$?
log "plasmashell exited status=${shell_status}"

kill "${kwin_pid}" >/dev/null 2>&1 || true
wait "${kwin_pid}" >/dev/null 2>&1 || true
exit "${shell_status}"
