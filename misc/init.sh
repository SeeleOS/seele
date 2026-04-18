#!/bin/sh

export PATH=/bin:/usr/bin
export TERM=xterm-256color
export HOME=/root

mkdir -p /run/dbus
if [ -x /usr/bin/dbus-daemon ] && [ ! -S /run/dbus/system_bus_socket ]; then
    if [ -f /usr/share/dbus-1/system.conf ] || [ -f /etc/dbus-1/system.conf ]; then
        /usr/bin/dbus-daemon --system --fork --nopidfile
    else
        /usr/bin/dbus-daemon \
            --session \
            --fork \
            --nopidfile \
            --address=unix:path=/run/dbus/system_bus_socket
    fi
fi

/bin/xinit /etc/X11/xinit/xinitrc -- /usr/bin/X :0 -dumbSched -extension GLX -extension DRI2 -extension DRI3
status=$?

echo "init: xinit exited with status ${status}"
exec /bin/bash
