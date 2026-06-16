use anyhow::{Context, Result};
use std::{fs, path::Path};
use xshell::{Shell, cmd};

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

pub fn write_repositories(sh: &Shell, repo_root: &Path, sysroot: &Path) -> Result<()> {
    let repositories = format!(
        "{mirror}/{branch}/main\n{mirror}/{branch}/community\n",
        mirror = ALPINE_MIRROR,
        branch = ALPINE_BRANCH,
    );
    let temp_file = repo_root.join(".seele-apk-repositories");
    fs::write(&temp_file, repositories)
        .with_context(|| format!("failed to write {}", temp_file.display()))?;

    let target = sysroot.join("etc/apk/repositories");
    cmd!(sh, "sudo install -D -m 0644 {temp_file} {target}").run()?;
    fs::remove_file(&temp_file).ok();
    Ok(())
}

pub fn install_packages(sh: &Shell, sysroot: &Path) -> Result<()> {
    cmd!(
        sh,
        "sudo apk --root {sysroot} --initdb --update-cache --allow-untrusted add {ALPINE_PACKAGES...}"
    )
    .run()?;
    Ok(())
}
