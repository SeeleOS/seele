#!/bin/sh

set -eu

cd "$(dirname "$0")/.."

if mountpoint -q sysroot; then
    exit 0
fi

sudo mount -o loop disk.img sysroot
