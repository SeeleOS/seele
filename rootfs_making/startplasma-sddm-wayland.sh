#!/bin/sh

set -eu

uid="$(id -u)"
runtime_dir="${XDG_RUNTIME_DIR:-/run/user/${uid}}"

export XDG_RUNTIME_DIR="${runtime_dir}"
: "${XDG_SESSION_TYPE:=wayland}"
: "${XDG_CURRENT_DESKTOP:=KDE}"
: "${DESKTOP_SESSION:=plasma}"
export XDG_SESSION_TYPE XDG_CURRENT_DESKTOP DESKTOP_SESSION

systemctl start "user-runtime-dir@${uid}.service" "user@${uid}.service"

bus_path="${runtime_dir}/bus"
for _ in $(seq 1 50); do
    if [ -S "${bus_path}" ]; then
        break
    fi
    sleep 0.1
done

if [ ! -S "${bus_path}" ]; then
    echo "startplasma-sddm-wayland: user bus not ready at ${bus_path}" >&2
    exit 1
fi

export DBUS_SESSION_BUS_ADDRESS="unix:path=${bus_path}"

if command -v dbus-update-activation-environment >/dev/null 2>&1; then
    dbus-update-activation-environment --systemd \
        DBUS_SESSION_BUS_ADDRESS \
        DESKTOP_SESSION \
        DISPLAY \
        HOME \
        LANG \
        LOGNAME \
        PATH \
        QT_WAYLAND_SHELL_INTEGRATION \
        USER \
        WAYLAND_DISPLAY \
        XAUTHORITY \
        XDG_CURRENT_DESKTOP \
        XDG_RUNTIME_DIR \
        XDG_SEAT \
        XDG_SESSION_CLASS \
        XDG_SESSION_DESKTOP \
        XDG_SESSION_PATH \
        XDG_SESSION_TYPE \
        XDG_VTNR \
        >/dev/null 2>&1 || true
fi

exec /usr/bin/startplasma-wayland
