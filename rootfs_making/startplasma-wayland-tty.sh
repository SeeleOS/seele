#!/bin/sh

set -eu

if [ -n "${DISPLAY:-}" ]; then
    echo "startplasma-wayland-tty: DISPLAY is set; refuse to start under X11" >&2
    exit 1
fi

tty_path="$(tty)"
case "${tty_path}" in
    /dev/tty[0-9]*)
        export XDG_VTNR="${tty_path#/dev/tty}"
        ;;
    *)
        echo "startplasma-wayland-tty: must be run from a local tty" >&2
        exit 1
        ;;
esac

runtime_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
if [ ! -d "${runtime_dir}" ]; then
    echo "startplasma-wayland-tty: missing XDG_RUNTIME_DIR ${runtime_dir}" >&2
    exit 1
fi

if [ ! -w "${runtime_dir}" ]; then
    echo "startplasma-wayland-tty: XDG_RUNTIME_DIR is not writable: ${runtime_dir}" >&2
    exit 1
fi

export XDG_RUNTIME_DIR="${runtime_dir}"
session_bus="${DBUS_SESSION_BUS_ADDRESS:-unix:path=${runtime_dir}/bus}"
session_bus_path="${session_bus#unix:path=}"
have_session_bus=0
if [ "${session_bus}" != "${session_bus_path}" ] && [ -S "${session_bus_path}" ]; then
    have_session_bus=1
    export DBUS_SESSION_BUS_ADDRESS="${session_bus}"
else
    unset DBUS_SESSION_BUS_ADDRESS
fi
export XDG_SESSION_TYPE=wayland
export XDG_SESSION_DESKTOP=KDE
export XDG_CURRENT_DESKTOP=KDE
export DESKTOP_SESSION=plasma
export KDE_FULL_SESSION=true
export QT_QPA_PLATFORM=wayland
export SDL_VIDEODRIVER=wayland
export XDG_SESSION_ID="${XDG_SESSION_ID:-}"
export XDG_SEAT="${XDG_SEAT:-}"

unset WAYLAND_DISPLAY
unset XAUTHORITY

if [ "${have_session_bus}" = "1" ] && command -v systemctl >/dev/null 2>&1; then
    systemctl --user import-environment \
        DBUS_SESSION_BUS_ADDRESS \
        XDG_RUNTIME_DIR \
        XDG_SESSION_ID \
        XDG_SEAT \
        XDG_SESSION_TYPE \
        XDG_SESSION_DESKTOP \
        XDG_CURRENT_DESKTOP \
        DESKTOP_SESSION \
        KDE_FULL_SESSION \
        QT_QPA_PLATFORM \
        SDL_VIDEODRIVER \
        XDG_VTNR || true
fi

if [ "${have_session_bus}" = "1" ] && command -v dbus-update-activation-environment >/dev/null 2>&1; then
    dbus-update-activation-environment --systemd \
        DBUS_SESSION_BUS_ADDRESS \
        XDG_RUNTIME_DIR \
        XDG_SESSION_ID \
        XDG_SEAT \
        XDG_SESSION_TYPE \
        XDG_SESSION_DESKTOP \
        XDG_CURRENT_DESKTOP \
        DESKTOP_SESSION \
        KDE_FULL_SESSION \
        QT_QPA_PLATFORM \
        SDL_VIDEODRIVER \
        XDG_VTNR || true
fi

exec /usr/lib/plasma-dbus-run-session-if-needed /usr/bin/startplasma-wayland
