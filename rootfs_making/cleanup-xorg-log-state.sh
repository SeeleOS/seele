#!/bin/sh

set -eu

if [ -d /var/log/Xorg.0.log.old ]; then
    rm -rf /var/log/Xorg.0.log.old
fi
