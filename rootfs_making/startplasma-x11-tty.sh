#!/bin/sh

set -eu

log_file=/var/log/autoplasma.log

log() {
    line="startplasma-x11-tty: $*"
    printf '%s\n' "${line}" >>"${log_file}"
    printf '%s\n' "${line}" >/dev/ttyS0 2>/dev/null || true
    sync
}

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
    log "missing runtime_dir=${runtime_dir}"
    echo "startplasma-x11-tty: missing XDG_RUNTIME_DIR ${runtime_dir}" >&2
    exit 1
fi

if [ ! -w "${runtime_dir}" ]; then
    log "runtime_dir_not_writable=${runtime_dir}"
    echo "startplasma-x11-tty: XDG_RUNTIME_DIR is not writable: ${runtime_dir}" >&2
    exit 1
fi

rm -f \
    /root/.config/kdeglobals \
    /root/.config/kdedefaults/kdeglobals \
    /root/.config/kdedefaults/plasmarc \
    /root/.config/kdedefaults/kcminputrc \
    /root/.config/kdedefaults/kwinrc \
    /root/.config/kdedefaults/ksplashrc \
    /root/.config/plasma-localerc

export XDG_RUNTIME_DIR="${runtime_dir}"
export XDG_SESSION_TYPE=x11
export XDG_SESSION_DESKTOP=KDE
export XDG_CURRENT_DESKTOP=KDE
export DESKTOP_SESSION=plasma
export KDE_FULL_SESSION=true

unset WAYLAND_DISPLAY
unset XAUTHORITY

log "exec startx /usr/bin/seele-plasma-x11-session -- :0 vt${XDG_VTNR} -nolisten tcp"
exec /usr/bin/startx /usr/bin/seele-plasma-x11-session -- :0 "vt${XDG_VTNR}" -nolisten tcp
