use anyhow::{Context, Result};
use std::{fs, path::Path};
use xshell::{Shell, cmd};

use crate::reporter::{WorkflowReporter, run_xshell_command};

const ARCH_PACKAGES: &[&str] = &[
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
    path: std::path::PathBuf,
}

impl PacmanConfig {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PacmanConfig {
    fn drop(&mut self) {
        fs::remove_file(&self.path).ok();
    }
}

pub fn create_pacman_config(repo_root: &Path) -> Result<PacmanConfig> {
    let path = repo_root.join(".seele-pacman.conf");
    fs::write(&path, PACMAN_CONF.trim_start())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(PacmanConfig { path })
}

pub fn install_packages(
    sh: &Shell,
    pacman_conf: &Path,
    rootfs_mount: &Path,
    reporter: &dyn WorkflowReporter,
) -> Result<()> {
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(
            sh,
            "sudo pacstrap -C {pacman_conf} -K -M {rootfs_mount} {ARCH_PACKAGES...}"
        ),
        reporter,
    )?;
    Ok(())
}

pub fn set_empty_root_password(
    sh: &Shell,
    rootfs_mount: &Path,
    reporter: &dyn WorkflowReporter,
) -> Result<()> {
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo chroot {rootfs_mount} /usr/bin/passwd -d root"),
        reporter,
    )?;
    Ok(())
}

pub fn configure_login_services(
    sh: &Shell,
    rootfs_mount: &Path,
    reporter: &dyn WorkflowReporter,
) -> Result<()> {
    let getty_wants = rootfs_mount
        .join("etc")
        .join("systemd")
        .join("system")
        .join("getty.target.wants");
    let systemd_system = rootfs_mount.join("etc").join("systemd").join("system");
    let default_target = systemd_system.join("default.target");
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(
            sh,
            "sudo ln -sfn /usr/lib/systemd/system/multi-user.target {default_target}"
        ),
        reporter,
    )?;
    fs::create_dir_all(&getty_wants)
        .with_context(|| format!("failed to create {}", getty_wants.display()))?;
    let tty1_getty = getty_wants.join("getty@tty1.service");
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(
            sh,
            "sudo ln -sfn /usr/lib/systemd/system/getty@.service {tty1_getty}"
        ),
        reporter,
    )?;
    Ok(())
}
