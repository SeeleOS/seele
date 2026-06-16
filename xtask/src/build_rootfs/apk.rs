use crate::build_rootfs::command::run;
use anyhow::{Context, Result};
use std::{fs, path::Path, process::Command};

const ALPINE_BRANCH: &str = "v3.24";
const ALPINE_MIRROR: &str = "https://dl-cdn.alpinelinux.org/alpine";
const ALPINE_PACKAGES: &[&str] = &[
    "alpine-keys",
    "alpine-base",
    "openrc",
    "busybox",
    "bash",
    "coreutils",
    "util-linux",
    "procps",
    "iproute2",
    "curl",
    "gcc",
    "musl-dev",
    "make",
    "pkgconf",
    "git",
    "rust",
    "cargo",
];

pub fn write_repositories(repo_root: &Path, sysroot: &Path) -> Result<()> {
    let repositories = format!(
        "{mirror}/{branch}/main\n{mirror}/{branch}/community\n",
        mirror = ALPINE_MIRROR,
        branch = ALPINE_BRANCH,
    );
    let temp_file = repo_root.join(".seele-apk-repositories");
    fs::write(&temp_file, repositories)
        .with_context(|| format!("failed to write {}", temp_file.display()))?;

    let target = sysroot.join("etc/apk/repositories");
    let install_result = run(Command::new("sudo")
        .arg("install")
        .arg("-D")
        .arg("-m")
        .arg("0644")
        .arg(&temp_file)
        .arg(&target));
    fs::remove_file(&temp_file).ok();
    install_result
}

pub fn install_packages(sysroot: &Path) -> Result<()> {
    let mut command = Command::new("sudo");
    command
        .arg("apk")
        .arg("--root")
        .arg(sysroot)
        .arg("--initdb")
        .arg("--update-cache")
        .arg("--allow-untrusted")
        .arg("add")
        .args(ALPINE_PACKAGES);
    run(&mut command)
}
