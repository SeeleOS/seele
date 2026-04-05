#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISK_IMG="${ROOT_DIR}/disk.img"
SYSROOT_DIR="${ROOT_DIR}/sysroot"

mkdir -p "${SYSROOT_DIR}"

if mountpoint -q "${SYSROOT_DIR}"; then
    sudo umount "${SYSROOT_DIR}"
fi

truncate -s 300M "${DISK_IMG}"
mkfs.ext4 -F "${DISK_IMG}"

sudo mount -o loop "${DISK_IMG}" "${SYSROOT_DIR}"

sudo mkdir -p "${SYSROOT_DIR}/tmp"
sudo chmod 1777 "${SYSROOT_DIR}/tmp"
sudo mkdir -p "${SYSROOT_DIR}/var/log"
sudo mkdir -p "${SYSROOT_DIR}/var/tmp"
sudo chmod 1777 "${SYSROOT_DIR}/var/tmp"

(
    cd "${ROOT_DIR}/relibc"
    make
)

(
    cd "${ROOT_DIR}/toolchain"
    ./install.rs
)

(
    cd "${ROOT_DIR}"
    cargo run -p packages -- install base
)
