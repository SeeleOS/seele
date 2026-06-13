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
export QT_QUICK_BACKEND=software
export QT_NO_XDG_DESKTOP_PORTAL=1
export LIBGL_ALWAYS_SOFTWARE=1
export GTK_USE_PORTAL=0
export GIO_USE_VFS=local
export KWIN_COMPOSE=N

unset WAYLAND_DISPLAY

mkdir -p "${XDG_CONFIG_HOME:-/root/.config}"
cat >"${XDG_CONFIG_HOME:-/root/.config}/startkderc" <<'EOF'
[General]
systemdBoot=false
EOF

log "exec startplasma-x11"
exec /usr/bin/startplasma-x11 >>"${log_file}" 2>&1
