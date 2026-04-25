#!/bin/sh

set -eu

socket_path="${SEELE_AGENT_TTY_SOCKET:-/tmp/seele-agent-tty.sock}"

if [ ! -S "$socket_path" ]; then
    echo "agent tty input socket not found: $socket_path" >&2
    exit 1
fi

if [ "${1:-}" = "--line" ]; then
    shift
    printf '%s\n' "$*" | nc -U -N "$socket_path"
    exit 0
fi

if [ "$#" -gt 0 ]; then
    printf '%s' "$*" | nc -U -N "$socket_path"
    exit 0
fi

if [ -t 0 ]; then
    old_stty="$(stty -g)"
    trap 'stty "$old_stty"' EXIT HUP INT TERM
    stty raw -echo
fi

nc -U "$socket_path"
