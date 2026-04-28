#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISK_IMG="${ROOT_DIR}/disk.img"
SYSROOT_DIR="${ROOT_DIR}/sysroot"
ROOTFS_MAKING_DIR="${ROOT_DIR}/rootfs_making"
PACMAN_CONF_TEMPLATE="${ROOTFS_MAKING_DIR}/pacman.conf"
PACMAN_CONF_IN_SYSROOT="${SYSROOT_DIR}/etc/pacman.conf"
OVERRIDE_DISK=0
ARCH_MIRROR="${ARCH_MIRROR:-https://mirrors.tuna.tsinghua.edu.cn/archlinux/\$repo/os/\$arch}"
PACSTRAP_BIN="$(command -v pacstrap)"
ARCH_CHROOT_BIN="$(command -v arch-chroot)"
AUR_BUILD_USER="aurbuilder"
AUR_BUILD_DIR="/var/tmp/aur-build"
ARCH_PACKAGES=(
    base
    base-devel
    rust
    bash
    niri
    hyprland
    clang
    nvim
    chromium
    firefox
    vscode
    alacritty
    sddm
    evtest
    libinput
    curl
    netsurf
    vim
    gcc
    busybox
    fastfetch
    iptables
    sudo
    xorg-server
    xorg-xinit
    xorg-xwayland
    xorg-xkbcomp
    xkeyboard-config
    xf86-input-evdev
    xf86-input-libinput
    fish
    yazi
    eza
    mesa
    seatd
    wayland
    weston
    xf86-video-fbdev
    xf86-video-vesa
    icewm
    plasma-meta
    plasma-x11-session
    konsole
    kalk
    dolphin
    nautilus
    gnome-calculator
    gnome-2048
    gnome-mines
    ttf-dejavu
    xorg-fonts-misc
)
AUR_PACKAGES=(
)

while [ $# -gt 0 ]; do
    case "$1" in
        --override)
            OVERRIDE_DISK=1
            ;;
        *)
            echo "unknown argument: $1" >&2
            echo "usage: $0 [--override]" >&2
            exit 1
            ;;
    esac
    shift
done

if [ -z "${PACSTRAP_BIN}" ] || [ -z "${ARCH_CHROOT_BIN}" ]; then
    echo "pacstrap and arch-chroot are required; run this script from the flake dev shell" >&2
    exit 1
fi

install_sysroot_file() {
    local source="$1"
    local target="$2"

    sudo rm -rf "${target}"
    sudo install -Dm644 "${source}" "${target}"
}

pacstrap_root() {
    sudo "${PACSTRAP_BIN}" \
        -C "${PACMAN_CONF_TEMPLATE}" \
        -c \
        -M \
        "${SYSROOT_DIR}" \
        "$@"
}

arch_chroot() {
    sudo "${ARCH_CHROOT_BIN}" "${SYSROOT_DIR}" "$@"
}

install_repo_packages() {
    if [ ! -x "${SYSROOT_DIR}/usr/bin/pacman" ]; then
        pacstrap_root "${ARCH_PACKAGES[@]}"
        return
    fi

    arch_chroot /usr/bin/pacman --noconfirm -Sy --needed "${ARCH_PACKAGES[@]}"
}

ensure_aur_builder() {
    arch_chroot /bin/sh -lc "
        set -eu
        if ! id -u '${AUR_BUILD_USER}' >/dev/null 2>&1; then
            useradd -m -U '${AUR_BUILD_USER}'
        fi
        install -d -m 0750 /etc/sudoers.d
        cat >/etc/sudoers.d/${AUR_BUILD_USER}-pacman <<'EOF'
${AUR_BUILD_USER} ALL=(ALL) NOPASSWD: /usr/bin/pacman
EOF
        chmod 0440 /etc/sudoers.d/${AUR_BUILD_USER}-pacman
        install -d -m 0777 '${AUR_BUILD_DIR}'
    "
}

install_aur_package() {
    local package="$1"
    local host_build_dir host_snapshot pkg_file host_pkg_file

    host_build_dir="$(mktemp -d)"
    host_snapshot="${host_build_dir}/${package}.tar.gz"
    trap 'rm -rf "${host_build_dir}"' RETURN

    curl \
        --fail \
        --location \
        --http1.1 \
        --retry 5 \
        --retry-all-errors \
        "https://aur.archlinux.org/cgit/aur.git/snapshot/${package}.tar.gz" \
        -o "${host_snapshot}"
    tar -C "${host_build_dir}" -xf "${host_snapshot}"

    if [ ! -d "${host_build_dir}/${package}" ]; then
        echo "AUR snapshot for ${package} did not contain ${package}/" >&2
        return 1
    fi

    ensure_aur_builder

    sudo rm -rf "${SYSROOT_DIR}${AUR_BUILD_DIR}/${package}"
    sudo cp -a "${host_build_dir}/${package}" "${SYSROOT_DIR}${AUR_BUILD_DIR}/"
    arch_chroot /usr/bin/chown -R "${AUR_BUILD_USER}:${AUR_BUILD_USER}" "${AUR_BUILD_DIR}/${package}"
    arch_chroot /usr/bin/runuser -u "${AUR_BUILD_USER}" -- \
        /bin/bash -lc "cd '${AUR_BUILD_DIR}/${package}' && /usr/bin/makepkg --noconfirm --syncdeps --clean --cleanbuild --force"

    pkg_file="$(
        arch_chroot /bin/sh -lc \
            "cd '${AUR_BUILD_DIR}/${package}' && /usr/bin/realpath ./*.pkg.tar.* | /usr/bin/head -n 1"
    )"

    if [ -z "${pkg_file}" ]; then
        echo "failed to locate built package for ${package}" >&2
        return 1
    fi

    host_pkg_file="${SYSROOT_DIR}${pkg_file}"
    if [ ! -f "${host_pkg_file}" ]; then
        echo "built package path does not exist on host: ${host_pkg_file}" >&2
        return 1
    fi

    arch_chroot /usr/bin/pacman --noconfirm --needed -U "${pkg_file}"
}

mkdir -p "${SYSROOT_DIR}"

if mountpoint -q "${SYSROOT_DIR}"; then
    sudo umount -l "${SYSROOT_DIR}"
fi

if [ -f "${DISK_IMG}" ]; then
    if [ "${OVERRIDE_DISK}" -eq 1 ]; then
        rm -f "${DISK_IMG}"
        truncate -s 10G "${DISK_IMG}"
        mkfs.ext4 -F "${DISK_IMG}"
    else
        echo "reusing existing disk image: ${DISK_IMG}"
    fi
else
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
sudo mkdir -p "${SYSROOT_DIR}/etc"
sudo rm -f "${SYSROOT_DIR}/var/lib/pacman/db.lck"
cat <<EOF | sudo tee "${PACMAN_CONF_TEMPLATE}" >/dev/null
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
sudo install -Dm644 "${PACMAN_CONF_TEMPLATE}" "${PACMAN_CONF_IN_SYSROOT}"

install_repo_packages
arch_chroot /bin/sh -lc "update-ca-trust || true"

sudo install -Dm644 "${ROOTFS_MAKING_DIR}/seatd.service" "${SYSROOT_DIR}/etc/systemd/system/seatd.service"
sudo install -d -m 0755 "${SYSROOT_DIR}/etc/systemd/system/systemd-localed.service.d"
install_sysroot_file "${ROOTFS_MAKING_DIR}/systemd-localed.service.d/override.conf" "${SYSROOT_DIR}/etc/systemd/system/systemd-localed.service.d/override.conf"
arch_chroot /usr/sbin/usermod -p '' root
arch_chroot /usr/bin/systemctl enable seatd.service

install_sysroot_file "${ROOTFS_MAKING_DIR}/xdg-runtime.sh" "${SYSROOT_DIR}/etc/profile.d/xdg-runtime.sh"

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
sudo rm -f "${SYSROOT_DIR}/init"

sudo install -Dm644 "${ROOT_DIR}/misc/maplemono.ttf" "${SYSROOT_DIR}/usr/share/fonts/TTF/maplemono.ttf"
sudo install -Dm644 "${ROOTFS_MAKING_DIR}/weston.ini" "${SYSROOT_DIR}/etc/xdg/weston/weston.ini"
sudo install -Dm755 "${ROOTFS_MAKING_DIR}/xinitrc" "${SYSROOT_DIR}/etc/X11/xinit/xinitrc"
sudo install -Dm755 "${ROOTFS_MAKING_DIR}/xinitrc" "${SYSROOT_DIR}/root/.xinitrc"
sudo install -Dm755 "${ROOTFS_MAKING_DIR}/startplasma-manual.sh" "${SYSROOT_DIR}/usr/bin/startplasma-manual.sh"

for package in "${AUR_PACKAGES[@]}"; do
    install_aur_package "${package}"
done
