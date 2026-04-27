#!/bin/sh

uid="$(id -u)"

if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
    export XDG_RUNTIME_DIR="/run/user/${uid}"
fi

if [ ! -d "${XDG_RUNTIME_DIR}" ]; then
    mkdir -p "${XDG_RUNTIME_DIR}"
fi

chmod 0700 "${XDG_RUNTIME_DIR}" 2>/dev/null || true
