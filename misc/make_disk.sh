#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISK_IMG="${ROOT_DIR}/disk.img"
SYSROOT_DIR="${ROOT_DIR}/sysroot"
PACMAN_CONF="${ROOT_DIR}/misc/pacman.conf"
ARCH_MIRROR="${ARCH_MIRROR:-https://mirrors.tuna.tsinghua.edu.cn/archlinux/\$repo/os/\$arch}"
ARCH_PACKAGES=(
    base
    bash
    gcc
    busybox
    fastfetch
    iptables
    xorg-server
    xorg-xinit
    xf86-input-libinput
    xf86-video-vesa
    icewm
)

mkdir -p "${SYSROOT_DIR}"

if mountpoint -q "${SYSROOT_DIR}"; then
    sudo umount "${SYSROOT_DIR}"
fi

if [ ! -f "${DISK_IMG}" ]; then
    truncate -s 10G "${DISK_IMG}"
    mkfs.ext4 -F "${DISK_IMG}"
fi

sudo mount -o loop "${DISK_IMG}" "${SYSROOT_DIR}"

sudo mkdir -p "${SYSROOT_DIR}/tmp"
sudo chmod 1777 "${SYSROOT_DIR}/tmp"
sudo mkdir -p "${SYSROOT_DIR}/var/log"
sudo mkdir -p "${SYSROOT_DIR}/var/tmp"
sudo chmod 1777 "${SYSROOT_DIR}/var/tmp"
sudo mkdir -p "${SYSROOT_DIR}/var/lib/pacman"
sudo mkdir -p "${SYSROOT_DIR}/var/cache/pacman/pkg"
cat <<EOF > "${PACMAN_CONF}"
[options]
Architecture = auto
CheckSpace
ParallelDownloads = 5
SigLevel = Never

[core]
Server = ${ARCH_MIRROR}

[extra]
Server = ${ARCH_MIRROR}
EOF

sudo pacman \
    --config "${PACMAN_CONF}" \
    --root "${SYSROOT_DIR}" \
    --dbpath "${SYSROOT_DIR}/var/lib/pacman" \
    --cachedir "${SYSROOT_DIR}/var/cache/pacman/pkg" \
    --noconfirm \
    --needed \
    -Sy \
    "${ARCH_PACKAGES[@]}"
