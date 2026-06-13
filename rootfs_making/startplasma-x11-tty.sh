#!/bin/sh

set -eu

log_file=/var/log/autoplasma.log

log() {
    line="startplasma-x11-tty: $*"
    printf '%s\n' "${line}" >>"${log_file}"
    printf '%s\n' "${line}" >/dev/ttyS0 2>/dev/null || true
    sync
}

: >"${log_file}" 2>/dev/null || true
log "enter pid=$$ uid=$(id -u) tty=$(tty 2>/dev/null || echo unknown)"

if [ -n "${DISPLAY:-}" ]; then
    log "refuse DISPLAY=${DISPLAY}"
    echo "startplasma-x11-tty: DISPLAY is set" >&2
    exit 1
fi

tty_path="$(tty)"
case "${tty_path}" in
    /dev/tty[0-9]*)
        export XDG_VTNR="${tty_path#/dev/tty}"
        log "using tty_path=${tty_path} vtnr=${XDG_VTNR}"
        ;;
    *)
        log "refuse tty_path=${tty_path}"
        echo "startplasma-x11-tty: must be run from a local tty" >&2
        exit 1
        ;;
esac

runtime_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
if [ ! -d "${runtime_dir}" ]; then
    log "creating runtime_dir=${runtime_dir}"
    mkdir -p "${runtime_dir}"
    chmod 0700 "${runtime_dir}"
fi

if [ ! -w "${runtime_dir}" ]; then
    log "runtime_dir_not_writable=${runtime_dir}"
    echo "startplasma-x11-tty: XDG_RUNTIME_DIR is not writable: ${runtime_dir}" >&2
    exit 1
fi

mkdir -p \
    /tmp/root-cache \
    /tmp/root-config \
    /tmp/root-local/share \
    /tmp/root-local/state \
    /tmp/root-runtime

rm -f \
    /tmp/root-config/kdeglobals \
    /tmp/root-config/kglobalshortcutsrc \
    /tmp/root-config/kdedefaults/kdeglobals \
    /tmp/root-config/kdedefaults/plasmarc \
    /tmp/root-config/kdedefaults/kcminputrc \
    /tmp/root-config/kdedefaults/kwinrc \
    /tmp/root-config/kdedefaults/ksplashrc \
    /tmp/root-config/kwinoutputconfig.json \
    /tmp/root-config/kwinrc \
    /tmp/root-config/kwinrc.lock \
    /tmp/root-config/plasma-localerc \
    /tmp/root-config/plasma-org.kde.plasma.desktop-appletsrc \
    /tmp/root-config/plasmashellrc \
    /tmp/root-local/state/kactivitymanagerdstaterc \
    /tmp/root-local/state/plasmashellstaterc

rm -rf \
    /tmp/root-cache/kwin \
    /tmp/root-cache/plasmashell \
    /tmp/root-local/share/kactivitymanagerd \
    /tmp/root-local/share/klipper

export XDG_RUNTIME_DIR="${runtime_dir}"
export XDG_CACHE_HOME=/tmp/root-cache
export XDG_CONFIG_HOME=/tmp/root-config
export XDG_DATA_HOME=/tmp/root-local/share
export XDG_STATE_HOME=/tmp/root-local/state
export XDG_SESSION_TYPE=x11
export XDG_SESSION_DESKTOP=KDE
export XDG_CURRENT_DESKTOP=KDE
export DESKTOP_SESSION=plasma
export KDE_FULL_SESSION=true
export KDE_SESSION_VERSION=6
export QT_QPA_PLATFORM=xcb
export QT_XCB_NO_MITSHM=1
export QT_QUICK_BACKEND=software
export LIBGL_ALWAYS_SOFTWARE=1
export GTK_USE_PORTAL=0
export GIO_USE_VFS=local
export KWIN_COMPOSE=N

unset WAYLAND_DISPLAY
unset XAUTHORITY

log "exec xinit /usr/bin/seele-plasma-x11-session -- /usr/bin/X :0 vt${XDG_VTNR} -nolisten tcp"
exec /usr/bin/xinit /usr/bin/seele-plasma-x11-session -- /usr/bin/X :0 "vt${XDG_VTNR}" -nolisten tcp >>"${log_file}" 2>&1
