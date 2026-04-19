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
    alacritty
    evtest
    libinput
    vim
    gcc
    busybox
    fastfetch
    iptables
    xorg-server
    xorg-xinit
    xorg-xkbcomp
    xkeyboard-config
    xf86-input-libinput
    fish
    yazi
    eza
    mesa
    xf86-video-fbdev
    xf86-video-vesa
    plasma-meta
    plasma-x11-session
    konsole
    dolphin
    ttf-dejavu
    xorg-fonts-misc
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
sudo mkdir -p "${SYSROOT_DIR}/etc/X11"
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

sudo chmod 0755 "${SYSROOT_DIR}/run"
sudo install -d -m 0755 "${SYSROOT_DIR}/run/dbus"
sudo install -d -m 0755 "${SYSROOT_DIR}/run/udev/data"
sudo install -d -m 0700 "${SYSROOT_DIR}/run/user/0"
sudo install -d -m 0700 "${SYSROOT_DIR}/root/.config"
sudo install -d -m 0700 "${SYSROOT_DIR}/root/.cache"
sudo install -d -m 0700 "${SYSROOT_DIR}/root/.local/share"
sudo install -d -m 0700 "${SYSROOT_DIR}/root/.local/state"
sudo install -d -m 1777 "${SYSROOT_DIR}/tmp/.X11-unix"
sudo install -d -m 0755 "${SYSROOT_DIR}/var/lib/dbus"

sudo install -Dm644 "${ROOT_DIR}/misc/maplemono.ttf" "${SYSROOT_DIR}/usr/share/fonts/TTF/maplemono.ttf"
sudo cp "${ROOT_DIR}/misc/xorg.conf" "${SYSROOT_DIR}/etc/X11/xorg.conf"
sudo install -Dm755 "${ROOT_DIR}/misc/xinitrc" "${SYSROOT_DIR}/etc/X11/xinit/xinitrc"
sudo install -Dm755 "${ROOT_DIR}/misc/startplasma-manual.sh" "${SYSROOT_DIR}/usr/bin/startplasma-manual.sh"
