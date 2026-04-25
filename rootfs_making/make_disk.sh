#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISK_IMG="${ROOT_DIR}/disk.img"
SYSROOT_DIR="${ROOT_DIR}/sysroot"
ROOTFS_MAKING_DIR="${ROOT_DIR}/rootfs_making"
PACMAN_CONF="${ROOTFS_MAKING_DIR}/pacman.conf"
HOST_PATH="${PATH}"
ARCH_MIRROR="${ARCH_MIRROR:-https://mirrors.tuna.tsinghua.edu.cn/archlinux/\$repo/os/\$arch}"
AUR_BUILD_USER="aurbuilder"
AUR_BUILD_DIR="/tmp/aur-build"
ARCH_PACKAGES=(
    base
    base-devel
    rust
    bash
    alacritty
    evtest
    libinput
    vim
    gcc
    busybox
    fastfetch
    iptables
    sudo
    xorg-server
    xorg-xinit
    xorg-xkbcomp
    xkeyboard-config
    xf86-input-evdev
    xf86-input-libinput
    fish
    yazi
    eza
    mesa
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
    st
)

install_sysroot_file() {
    local source="$1"
    local target="$2"

    sudo rm -rf "${target}"
    sudo install -Dm644 "${source}" "${target}"
}

pacman_root() {
    sudo env "PATH=${HOST_PATH}" pacman \
        --config "${PACMAN_CONF}" \
        --root "${SYSROOT_DIR}" \
        --dbpath "${SYSROOT_DIR}/var/lib/pacman" \
        --cachedir "${SYSROOT_DIR}/var/cache/pacman/pkg" \
        --noconfirm \
        "$@"
}

mount_chroot_api_fs() {
    sudo install -d -m 0755 "${SYSROOT_DIR}/dev" "${SYSROOT_DIR}/dev/pts" "${SYSROOT_DIR}/proc" "${SYSROOT_DIR}/sys"

    if ! mountpoint -q "${SYSROOT_DIR}/dev"; then
        sudo mount --bind /dev "${SYSROOT_DIR}/dev"
    fi

    if ! mountpoint -q "${SYSROOT_DIR}/dev/pts"; then
        sudo mount --bind /dev/pts "${SYSROOT_DIR}/dev/pts"
    fi

    if ! mountpoint -q "${SYSROOT_DIR}/proc"; then
        sudo mount -t proc proc "${SYSROOT_DIR}/proc"
    fi

    if ! mountpoint -q "${SYSROOT_DIR}/sys"; then
        sudo mount --rbind /sys "${SYSROOT_DIR}/sys"
    fi
}

umount_chroot_api_fs() {
    if mountpoint -q "${SYSROOT_DIR}/dev/pts"; then
        sudo umount -l "${SYSROOT_DIR}/dev/pts" || true
    fi

    if mountpoint -q "${SYSROOT_DIR}/dev"; then
        sudo umount -l "${SYSROOT_DIR}/dev" || true
    fi

    if mountpoint -q "${SYSROOT_DIR}/proc"; then
        sudo umount -l "${SYSROOT_DIR}/proc" || true
    fi

    if mountpoint -q "${SYSROOT_DIR}/sys"; then
        sudo umount -R -l "${SYSROOT_DIR}/sys" || true
    fi
}

trap umount_chroot_api_fs EXIT

ensure_aur_builder() {
    sudo chroot "${SYSROOT_DIR}" /bin/sh -lc "
        set -eu
        if ! id -u '${AUR_BUILD_USER}' >/dev/null 2>&1; then
            useradd -m -U '${AUR_BUILD_USER}'
        fi
        install -d -m 0755 /etc/sudoers.d
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
    mount_chroot_api_fs
    sudo chroot "${SYSROOT_DIR}" /usr/bin/chown -R "${AUR_BUILD_USER}:${AUR_BUILD_USER}" "${AUR_BUILD_DIR}/${package}"

    sudo chroot "${SYSROOT_DIR}" /usr/bin/runuser -u "${AUR_BUILD_USER}" -- \
        /bin/bash -lc "cd '${AUR_BUILD_DIR}/${package}' && /usr/bin/makepkg --noconfirm --syncdeps --clean --cleanbuild --force"

    pkg_file="$(
        sudo chroot "${SYSROOT_DIR}" /bin/sh -lc \
            "cd '${AUR_BUILD_DIR}/${package}' && /usr/bin/realpath ./*.pkg.tar.* | /usr/bin/head -n 1"
    )"

    umount_chroot_api_fs

    if [ -z "${pkg_file}" ]; then
        echo "failed to locate built package for ${package}" >&2
        return 1
    fi

    host_pkg_file="${SYSROOT_DIR}${pkg_file}"
    if [ ! -f "${host_pkg_file}" ]; then
        echo "built package path does not exist on host: ${host_pkg_file}" >&2
        return 1
    fi

    pacman_root --needed -U "${host_pkg_file}"
}

mkdir -p "${SYSROOT_DIR}"

if mountpoint -q "${SYSROOT_DIR}"; then
    umount_chroot_api_fs
    sudo umount -l "${SYSROOT_DIR}"
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
sudo rm -f "${SYSROOT_DIR}/var/lib/pacman/db.lck"
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

pacman_root --needed -Sy "${ARCH_PACKAGES[@]}"

for package in "${AUR_PACKAGES[@]}"; do
    install_aur_package "${package}"
done

sudo chroot "${SYSROOT_DIR}" /usr/bin/passwd -d root

install_sysroot_file "${ROOTFS_MAKING_DIR}/locale.conf" "${SYSROOT_DIR}/etc/locale.conf"
install_sysroot_file "${ROOTFS_MAKING_DIR}/vconsole.conf" "${SYSROOT_DIR}/etc/vconsole.conf"
install_sysroot_file "${ROOTFS_MAKING_DIR}/locale.sh" "${SYSROOT_DIR}/etc/profile.d/locale.sh"

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
sudo cp "${ROOTFS_MAKING_DIR}/xorg.conf" "${SYSROOT_DIR}/etc/X11/xorg.conf"
sudo install -Dm755 "${ROOTFS_MAKING_DIR}/xinitrc" "${SYSROOT_DIR}/etc/X11/xinit/xinitrc"
sudo install -Dm755 "${ROOTFS_MAKING_DIR}/root.xinitrc" "${SYSROOT_DIR}/root/.xinitrc"
sudo install -Dm755 "${ROOTFS_MAKING_DIR}/root.bash_profile" "${SYSROOT_DIR}/root/.bash_profile"
sudo install -Dm755 "${ROOTFS_MAKING_DIR}/startplasma-manual.sh" "${SYSROOT_DIR}/usr/bin/startplasma-manual.sh"
sudo install -Dm644 "${ROOTFS_MAKING_DIR}/getty-autologin.conf" "${SYSROOT_DIR}/etc/systemd/system/getty@tty1.service.d/autologin.conf"
