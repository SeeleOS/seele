#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISK_IMG="${ROOT_DIR}/disk.img"
SYSROOT_DIR="${ROOT_DIR}/sysroot"
ALPINE_BRANCH="${ALPINE_BRANCH:-latest-stable}"
ALPINE_MIRROR="${ALPINE_MIRROR:-https://mirrors.tuna.tsinghua.edu.cn/alpine}"
APK_PACKAGES=(
    alpine-base
    gcc
    busybox
    xorg-server
    xinit
    xf86-input-libinput
    xf86-video-vesa
    icewm
)

mkdir -p "${SYSROOT_DIR}"

if mountpoint -q "${SYSROOT_DIR}"; then
    sudo umount "${SYSROOT_DIR}"
fi

sudo rm -f "${DISK_IMG}"
truncate -s 10G "${DISK_IMG}"
mkfs.ext4 -F "${DISK_IMG}"

sudo mount -o loop "${DISK_IMG}" "${SYSROOT_DIR}"

sudo mkdir -p "${SYSROOT_DIR}/tmp"
sudo chmod 1777 "${SYSROOT_DIR}/tmp"
sudo mkdir -p "${SYSROOT_DIR}/var/log"
sudo mkdir -p "${SYSROOT_DIR}/var/tmp"
sudo chmod 1777 "${SYSROOT_DIR}/var/tmp"
sudo mkdir -p "${SYSROOT_DIR}/etc/apk"
cat <<EOF | sudo tee "${SYSROOT_DIR}/etc/apk/repositories" >/dev/null
${ALPINE_MIRROR}/${ALPINE_BRANCH}/main
${ALPINE_MIRROR}/${ALPINE_BRANCH}/community
EOF

sudo apk add \
    --root "${SYSROOT_DIR}" \
    --initdb \
    --allow-untrusted \
    --update-cache \
    "${APK_PACKAGES[@]}"
