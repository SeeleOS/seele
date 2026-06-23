use crate::{ControlContext, process::ProcessRunner};
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
    context: &dyn ControlContext,
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
    context: &dyn ControlContext,
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
    context: &dyn ControlContext,
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
