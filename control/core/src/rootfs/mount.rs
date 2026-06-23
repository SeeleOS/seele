use super::paths::{RootfsPaths, paths};
use crate::{ControlContext, process::ProcessRunner};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

pub fn ensure_mounted(repo: &Path, context: &dyn ControlContext) -> Result<RootfsPaths> {
    let paths = paths(repo);
    let runner = ProcessRunner::new(&paths.artifact_dir)?;
    ensure_mounted_with_runner(&runner, context, &paths)?;
    Ok(paths)
}

pub fn unmount(repo: &Path, context: &dyn ControlContext) -> Result<i32> {
    let paths = paths(repo);
    let runner = ProcessRunner::new(&paths.artifact_dir)?;
    unmount_with_runner(&runner, context, &paths.mount)?;
    Ok(0)
}

pub(super) fn ensure_mounted_with_runner(
    runner: &ProcessRunner,
    context: &dyn ControlContext,
    paths: &RootfsPaths,
) -> Result<()> {
    fs::create_dir_all(&paths.mount)
        .with_context(|| format!("failed to create {}", paths.mount.display()))?;
    if is_mounted(&paths.mount)? {
        return Ok(());
    }
    if !paths.image.exists() {
        bail!("rootfs image does not exist: {}", paths.image.display());
    }
    runner.run_success(
        context,
        "rootfs_mount",
        Command::new("sudo")
            .arg("mount")
            .arg("-o")
            .arg("loop")
            .arg(&paths.image)
            .arg(&paths.mount),
    )?;
    Ok(())
}

pub(super) fn unmount_with_runner(
    runner: &ProcessRunner,
    context: &dyn ControlContext,
    mount: &Path,
) -> Result<()> {
    if is_mounted(mount)? {
        runner.run_success(
            context,
            "rootfs_umount",
            Command::new("sudo").arg("umount").arg(mount),
        )?;
    }
    Ok(())
}

fn is_mounted(path: &Path) -> Result<bool> {
    Ok(Command::new("mountpoint")
        .arg("-q")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to inspect rootfs mountpoint")?
        .success())
}
