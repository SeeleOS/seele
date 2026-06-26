use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};
use xshell::{Shell, cmd};

pub const ARCH_PACKAGES: &[&str] = &[
    "base",
    "base-devel",
    "systemd",
    "iptables",
    "busybox",
    "fish",
    "yazi",
    "clang",
    "vim",
    "nvim",
    "weston",
    "hyprland",
    "niri",
    "sway",
    "bash",
    "coreutils",
    "util-linux",
    "fastfetch",
    "alacritty",
    "procps",
    "iproute2",
    "curl",
    "e2fsprogs",
    "gcc",
    "glibc",
    "make",
    "pkgconf",
    "git",
    "rust",
    "pacman",
    "fakeroot",
    "autoconf",
    "automake",
    "acl",
    "gawk",
    "libcap",
    "perl",
    "numactl",
    "libaio",
    "libmnl",
    "python",
    "openssl",
    "libtirpc",
    "plasma-meta",
    "dolphin",
    "konsole",
    "kate",
    "gnome-shell",
    "gnome-control-center",
    "gnome-terminal",
    "nautilus",
    "firefox",
    "chromium",
    "code",
    "mesa",
    "vulkan-virtio",
    "pipewire",
    "wireplumber",
    "networkmanager",
    "sudo",
    "waybar",
    "xdg-desktop-portal",
    "xdg-desktop-portal-gtk",
    "xdg-desktop-portal-kde",
    "xdg-desktop-portal-hyprland",
    "ttf-dejavu",
    "noto-fonts",
    "noto-fonts-cjk",
    "noto-fonts-emoji",
];

const PACMAN_CONF: &str = r#"
[options]
Architecture = auto
SigLevel = Never
LocalFileSigLevel = Optional
ParallelDownloads = 5

[core]
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
Server = https://mirror.rackspace.com/archlinux/$repo/os/$arch

[extra]
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
Server = https://mirror.rackspace.com/archlinux/$repo/os/$arch

[multilib]
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
Server = https://mirror.rackspace.com/archlinux/$repo/os/$arch
"#;

const KMOD_WRAPPER: &str = r#"#!/bin/sh
command="$1"
if [ "$1" = "load" ] || [ "$1" = "modprobe" ]; then
    shift
    for arg in "$@"; do
        case "$arg" in
            -*)
                ;;
            dns_resolver)
                exit 0
                ;;
            *)
                break
                ;;
        esac
    done
fi
if [ "$command" = "dns_resolver" ]; then
    exit 0
fi
if [ "$command" = "load" ] || [ "$command" = "modprobe" ]; then
    exec /usr/bin/kmod.real "$command" "$@"
fi
exec /usr/bin/kmod.real "$@"
"#;

pub struct PacmanConfig {
    path: PathBuf,
}

impl PacmanConfig {
    pub fn create(repo: &Path) -> Result<Self> {
        let path = repo.join(".seele-pacman.conf");
        fs::write(&path, PACMAN_CONF.trim_start())
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PacmanConfig {
    fn drop(&mut self) {
        fs::remove_file(&self.path).ok();
    }
}

pub fn install_packages(sh: &Shell, pacman_conf: &Path, rootfs_mount: &Path) -> Result<()> {
    cmd!(
        sh,
        "sudo pacstrap -C {pacman_conf} -K -M {rootfs_mount} {ARCH_PACKAGES...}"
    )
    .run()?;
    Ok(())
}

pub fn set_empty_root_password(sh: &Shell, rootfs_mount: &Path) -> Result<()> {
    cmd!(sh, "sudo chroot {rootfs_mount} /usr/bin/passwd -d root").run()?;
    Ok(())
}

pub fn configure_login_services(sh: &Shell, rootfs_mount: &Path) -> Result<()> {
    let getty_wants = rootfs_mount
        .join("etc")
        .join("systemd")
        .join("system")
        .join("getty.target.wants");
    let systemd_system = rootfs_mount.join("etc").join("systemd").join("system");
    let default_target = systemd_system.join("default.target");
    fs::create_dir_all(&getty_wants)
        .with_context(|| format!("failed to create {}", getty_wants.display()))?;
    cmd!(
        sh,
        "sudo ln -sfn /usr/lib/systemd/system/multi-user.target {default_target}"
    )
    .run()?;
    let tty1_getty = getty_wants.join("getty@tty1.service");
    cmd!(
        sh,
        "sudo ln -sfn /usr/lib/systemd/system/getty@.service {tty1_getty}"
    )
    .run()?;
    Ok(())
}

pub fn install_modprobe_wrapper(sh: &Shell, rootfs_mount: &Path) -> Result<()> {
    let kmod = rootfs_mount.join("usr/bin/kmod");
    let real_kmod = rootfs_mount.join("usr/bin/kmod.real");
    if !real_kmod.exists() {
        cmd!(sh, "sudo mv {kmod} {real_kmod}").run()?;
    }
    let wrapper_source = std::env::temp_dir().join("seele-kmod-wrapper");
    fs::write(&wrapper_source, KMOD_WRAPPER)
        .with_context(|| format!("failed to write {}", wrapper_source.display()))?;
    cmd!(sh, "sudo install -m 0755 {wrapper_source} {kmod}").run()?;
    fs::remove_file(&wrapper_source).ok();
    Ok(())
}
