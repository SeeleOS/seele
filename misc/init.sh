#!/bin/sh

export PATH=/bin:/usr/bin
export TERM=xterm-256color
export HOME=/root
export USER=root
export LOGNAME=root
export SHELL=/bin/bash
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export XDG_RUNTIME_DIR=/run/user/0
export XDG_CONFIG_HOME=/root/.config
export XDG_CACHE_HOME=/root/.cache
export XDG_DATA_HOME=/root/.local/share
export XDG_STATE_HOME=/root/.local/state
export XDG_SESSION_TYPE=x11
export XDG_SESSION_CLASS=user
export XDG_SESSION_DESKTOP=KDE
export XDG_CURRENT_DESKTOP=KDE
export DESKTOP_SESSION=plasma
export KDE_FULL_SESSION=true
export KDE_SESSION_VERSION=6
export QT_LOGGING_RULES="kwin_*.debug=true;kf.plasma*.debug=true;qml.debug=true"
export QT_MESSAGE_PATTERN="qt[%{type}] %{category}: %{message}"
export QT_XCB_FORCE_SOFTWARE_OPENGL=1
export QT_X11_NO_MITSHM=1
export LIBGL_ALWAYS_SOFTWARE=1
export GALLIUM_DRIVER=llvmpipe

mkdir -p /var/log /run/dbus
rm -f /tmp/.X0-lock /tmp/.X11-unix/X0

cat <<'EOF' >/run/udev/data/c13:64
I:1
E:ID_INPUT=1
E:ID_INPUT_KEY=1
E:ID_INPUT_KEYBOARD=1
E:ID_SEAT=seat0
E:WL_SEAT=seat0
E:LIBINPUT_DEVICE_GROUP=seele-keyboard
V:1
EOF

cat <<'EOF' >/run/udev/data/+input:input0
I:1
E:ID_INPUT=1
E:ID_INPUT_KEY=1
E:ID_INPUT_KEYBOARD=1
E:ID_SEAT=seat0
E:WL_SEAT=seat0
E:LIBINPUT_DEVICE_GROUP=seele-keyboard
V:1
EOF

cat <<'EOF' >/run/udev/data/+input:input
I:1
V:1
EOF

cat <<'EOF' >/run/udev/data/c13:65
I:1
E:ID_INPUT=1
E:ID_INPUT_MOUSE=1
E:ID_SEAT=seat0
E:WL_SEAT=seat0
E:LIBINPUT_DEVICE_GROUP=seele-mouse
V:1
EOF

cat <<'EOF' >/run/udev/data/+input:input1
I:1
E:ID_INPUT=1
E:ID_INPUT_MOUSE=1
E:ID_SEAT=seat0
E:WL_SEAT=seat0
E:LIBINPUT_DEVICE_GROUP=seele-mouse
V:1
EOF

cat <<'EOF' >/run/udev/data/+serio:serio0
I:1
V:1
EOF

cat <<'EOF' >/run/udev/data/+serio:serio1
I:1
V:1
EOF

cat <<'EOF' >/run/udev/data/+platform:i8042
I:1
V:1
EOF

cat <<'EOF' >/run/udev/data/+platform:platform
I:1
V:1
EOF

cat <<'EOF' >/run/udev/data/+devices:devices
I:1
V:1
EOF

if [ -x /usr/bin/dbus-uuidgen ]; then
    /usr/bin/dbus-uuidgen --ensure=/etc/machine-id
    if [ ! -f /var/lib/dbus/machine-id ]; then
        cp /etc/machine-id /var/lib/dbus/machine-id
    fi
fi

start_dbus_daemon() {
    name="$1"
    socket_path="$2"
    log_path="$3"
    shift 3

    rm -f "$socket_path"
    echo "init: starting ${name} dbus-daemon"
    "$@" >"$log_path" 2>&1 &
    daemon_pid=$!
    echo "init: ${name} dbus-daemon pid=${daemon_pid}"
}

if [ -x /usr/bin/dbus-daemon ]; then
    export DBUS_SYSTEM_BUS_ADDRESS="unix:path=/run/dbus/system_bus_socket"
    start_dbus_daemon \
        system \
        /run/dbus/system_bus_socket \
        /var/log/dbus-system.log \
        /usr/bin/dbus-daemon --system --nofork --nopidfile

    export DBUS_SESSION_BUS_ADDRESS="unix:path=${XDG_RUNTIME_DIR}/bus"
    start_dbus_daemon \
        session \
        "${XDG_RUNTIME_DIR}/bus" \
        /var/log/dbus-session.log \
        /usr/bin/dbus-daemon --session --nofork --nopidfile --address="${DBUS_SESSION_BUS_ADDRESS}"
fi

echo "init: launching plasma x11 session"
/bin/xinit /etc/X11/xinit/xinitrc -- /usr/bin/X :0 -nolisten tcp -extension MIT-SHM
status=$?

echo "init: xinit exited with status ${status}"
exec /bin/bash
