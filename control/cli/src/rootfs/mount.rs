use super::paths::{RootfsPaths, paths};
use anyhow::{Result, bail};
use std::path::Path;
use xshell::{Shell, cmd};

pub fn ensure_mounted_repo(sh: &Shell, repo: &Path) -> Result<RootfsPaths> {
    let paths = paths(repo);
    ensure_mounted(sh, &paths)?;
    Ok(paths)
}

pub fn unmount_repo(sh: &Shell, repo: &Path) -> Result<i32> {
    let paths = paths(repo);
    unmount(sh, &paths.mount)?;
    Ok(0)
}

pub(super) fn ensure_mounted(sh: &Shell, paths: &RootfsPaths) -> Result<()> {
    sh.create_dir(&paths.mount)?;
    if is_mounted(&paths.mount)? {
        return Ok(());
    }
    if !paths.image.exists() {
        bail!("rootfs image does not exist: {}", paths.image.display());
    }
    let image = &paths.image;
    let mount = &paths.mount;
    cmd!(sh, "sudo mount -o loop {image} {mount}").run()?;
    Ok(())
}

pub(super) fn unmount(sh: &Shell, mount: &Path) -> Result<()> {
    if is_mounted(mount)? {
        cmd!(sh, "sudo umount {mount}").run()?;
    }
    Ok(())
}

fn is_mounted(path: &Path) -> Result<bool> {
    let sh = Shell::new()?;
    Ok(cmd!(sh, "mountpoint -q {path}").quiet().run().is_ok())
}
