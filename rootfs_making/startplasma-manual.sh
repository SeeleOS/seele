#!/bin/sh

set -eu

export DESKTOP_SESSION=plasma
export XDG_SESSION_DESKTOP=KDE
export XDG_CURRENT_DESKTOP=KDE
export KDE_FULL_SESSION=true
export KDE_SESSION_VERSION=6

log_dir=/var/log
log_file="${log_dir}/startplasma.log"

mkdir -p "$log_dir"
exec >>"$log_file" 2>&1

echo "manual plasma: begin"

run_bg() {
    name="$1"
    shift
    echo "manual plasma: starting ${name}: $*"
    "$@" >"${log_dir}/${name}.log" 2>&1 &
    echo "manual plasma: ${name} pid=$!"
}

if command -v xsetroot >/dev/null 2>&1; then
    xsetroot -solid black
fi

if [ -x /usr/bin/kwin_x11 ]; then
    run_bg kwin_x11 /usr/bin/kwin_x11 --replace
fi

if [ -x /usr/bin/kded6 ]; then
    run_bg kded6 /usr/bin/kded6
fi

if [ -x /usr/bin/ksmserver ]; then
    run_bg ksmserver /usr/bin/ksmserver
fi

if [ -x /usr/bin/kcminit_startup ]; then
    run_bg kcminit_startup /usr/bin/kcminit_startup
fi

echo "manual plasma: exec plasmashell"
exec /bin/sh -c '/usr/bin/plasmashell 2>&1 | tee /var/log/plasmashell.log'
