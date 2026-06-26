use crate::{JobContext, process::ProcessRunner};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

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

pub fn install_packages(
    runner: &ProcessRunner,
    context: &JobContext,
    pacman_conf: &Path,
    rootfs_mount: &Path,
) -> Result<()> {
    runner.run_success(
        context,
        "pacstrap_base",
        Command::new("sudo")
            .arg("pacstrap")
            .arg("-C")
            .arg(pacman_conf)
            .arg("-K")
            .arg("-M")
            .arg(rootfs_mount)
            .args(ARCH_PACKAGES),
    )?;
    Ok(())
}

pub fn set_empty_root_password(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
) -> Result<()> {
    runner.run_success(
        context,
        "rootfs_empty_root_password",
        Command::new("sudo")
            .arg("chroot")
            .arg(rootfs_mount)
            .arg("/usr/bin/passwd")
            .arg("-d")
            .arg("root"),
    )?;
    Ok(())
}

pub fn configure_login_services(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
) -> Result<()> {
    let getty_wants = rootfs_mount
        .join("etc")
        .join("systemd")
        .join("system")
        .join("getty.target.wants");
    let systemd_system = rootfs_mount.join("etc").join("systemd").join("system");
    let default_target = systemd_system.join("default.target");
    fs::create_dir_all(&getty_wants)
        .with_context(|| format!("failed to create {}", getty_wants.display()))?;
    runner.run_success(
        context,
        "rootfs_default_target",
        Command::new("sudo")
            .arg("ln")
            .arg("-sfn")
            .arg("/usr/lib/systemd/system/multi-user.target")
            .arg(&default_target),
    )?;
    runner.run_success(
        context,
        "rootfs_tty1_getty",
        Command::new("sudo")
            .arg("ln")
            .arg("-sfn")
            .arg("/usr/lib/systemd/system/getty@.service")
            .arg(getty_wants.join("getty@tty1.service")),
    )?;
    Ok(())
}

pub fn install_modprobe_wrapper(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
) -> Result<()> {
    let kmod = rootfs_mount.join("usr/bin/kmod");
    let real_kmod = rootfs_mount.join("usr/bin/kmod.real");
    if !real_kmod.exists() {
        runner.run_success(
            context,
            "rootfs_preserve_real_kmod",
            Command::new("sudo").arg("mv").arg(&kmod).arg(&real_kmod),
        )?;
    }
    let wrapper_source = runner.artifact_dir().join("kmod-wrapper");
    fs::write(&wrapper_source, KMOD_WRAPPER)
        .with_context(|| format!("failed to write {}", wrapper_source.display()))?;
    runner.run_success(
        context,
        "rootfs_install_kmod_wrapper",
        Command::new("sudo")
            .arg("install")
            .arg("-m")
            .arg("0755")
            .arg(&wrapper_source)
            .arg(&kmod),
    )?;
    Ok(())
}
