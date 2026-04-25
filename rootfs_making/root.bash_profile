#!/bin/bash

if [ -f ~/.bashrc ]; then
    . ~/.bashrc
fi

if [ -z "${DISPLAY:-}" ] && [ "${XDG_VTNR:-}" = "1" ] && [ "$(tty)" = "/dev/tty1" ]; then
    xinit
fi
