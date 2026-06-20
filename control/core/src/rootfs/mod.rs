use crate::{
    Artifact, ArtifactKind, Event, JobContext, RootfsEvent, StepState, process::ProcessRunner,
    target_dir,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildRootfsConfig {
    pub override_rootfs: bool,
    pub rebuild_aur: bool,
    pub rebuild_aur_packages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootfsPaths {
    pub image: PathBuf,
    pub mount: PathBuf,
    pub artifact_dir: PathBuf,
}

pub fn paths(repo: &Path) -> RootfsPaths {
    let target = target_dir(repo);
    RootfsPaths {
        image: target.join("rootfs.img"),
        mount: target.join("rootfs_mnt"),
        artifact_dir: target.join("control-artifacts").join("rootfs"),
    }
}

pub fn build_rootfs(repo: &Path, config: &BuildRootfsConfig, context: &JobContext) -> Result<i32> {
    let paths = paths(repo);
    fs::create_dir_all(&paths.mount)
        .with_context(|| format!("failed to create {}", paths.mount.display()))?;
    let runner = ProcessRunner::new(&paths.artifact_dir)?;

    run_step(context, "prepare_image", || {
        if config.override_rootfs && paths.image.exists() {
            fs::remove_file(&paths.image)
                .with_context(|| format!("failed to remove {}", paths.image.display()))?;
        }
        if !paths.image.exists() {
            runner.run_success(
                context,
                "rootfs_truncate",
                Command::new("truncate")
                    .arg("-s")
                    .arg("16G")
                    .arg(&paths.image),
            )?;
            runner.run_success(
                context,
                "rootfs_mkfs",
                Command::new("mkfs.ext4").arg("-F").arg(&paths.image),
            )?;
        }
        Ok(())
    })?;

    run_step(context, "mount", || {
        ensure_mounted_with_runner(&runner, context, &paths)
    })?;
    run_step(context, "install_base", || {
        runner.run_success(
            context,
            "pacstrap_base",
            Command::new("sudo")
                .arg("pacstrap")
                .arg("-M")
                .arg(&paths.mount)
                .args(["base", "bash", "coreutils", "util-linux", "procps-ng"]),
        )?;
        Ok(())
    })?;
    run_step(context, "install_aur", || {
        if config.rebuild_aur || !config.rebuild_aur_packages.is_empty() {
            context.event(Event::Rootfs(RootfsEvent {
                step: "install_aur".to_string(),
                state: StepState::Skipped,
                message: "AUR rebuild requested; package-specific implementation is intentionally not stubbed in the new control plane yet".to_string(),
            }));
            bail!("AUR package build is not implemented in the new control plane");
        }
        Ok(())
    })?;
    run_step(context, "install_kirk_ltp", || {
        fs::create_dir_all(paths.mount.join("opt/seele-tests"))
            .context("failed to create test directory in rootfs")?;
        Ok(())
    })?;
    run_step(context, "configure", || {
        fs::create_dir_all(paths.mount.join("var/log")).context("failed to create var/log")?;
        fs::create_dir_all(paths.mount.join("tmp")).context("failed to create tmp")?;
        Ok(())
    })?;
    run_step(context, "finalize", || {
        context.artifact(Artifact {
            kind: ArtifactKind::RootfsImage,
            path: paths.image.clone(),
            description: "Arch rootfs image".to_string(),
        });
        Ok(())
    })?;
    run_step(context, "unmount", || {
        unmount_with_runner(&runner, context, &paths.mount)
    })?;
    Ok(0)
}

pub fn ensure_mounted(repo: &Path, context: &JobContext) -> Result<RootfsPaths> {
    let paths = paths(repo);
    let runner = ProcessRunner::new(&paths.artifact_dir)?;
    ensure_mounted_with_runner(&runner, context, &paths)?;
    Ok(paths)
}

pub fn unmount(repo: &Path, context: &JobContext) -> Result<i32> {
    let paths = paths(repo);
    let runner = ProcessRunner::new(&paths.artifact_dir)?;
    unmount_with_runner(&runner, context, &paths.mount)?;
    Ok(0)
}

fn ensure_mounted_with_runner(
    runner: &ProcessRunner,
    context: &JobContext,
    paths: &RootfsPaths,
) -> Result<()> {
    fs::create_dir_all(&paths.mount)
        .with_context(|| format!("failed to create {}", paths.mount.display()))?;
    let mounted = Command::new("mountpoint")
        .arg("-q")
        .arg(&paths.mount)
        .status()
        .context("failed to inspect rootfs mountpoint")?
        .success();
    if mounted {
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

fn unmount_with_runner(runner: &ProcessRunner, context: &JobContext, mount: &Path) -> Result<()> {
    let mounted = Command::new("mountpoint")
        .arg("-q")
        .arg(mount)
        .status()
        .context("failed to inspect rootfs mountpoint")?
        .success();
    if mounted {
        runner.run_success(
            context,
            "rootfs_umount",
            Command::new("sudo").arg("umount").arg(mount),
        )?;
    }
    Ok(())
}

fn run_step(context: &JobContext, step: &str, f: impl FnOnce() -> Result<()>) -> Result<()> {
    context.event(Event::Rootfs(RootfsEvent {
        step: step.to_string(),
        state: StepState::Started,
        message: "started".to_string(),
    }));
    match f() {
        Ok(()) => {
            context.event(Event::Rootfs(RootfsEvent {
                step: step.to_string(),
                state: StepState::Finished,
                message: "finished".to_string(),
            }));
            Ok(())
        }
        Err(err) => {
            context.event(Event::Rootfs(RootfsEvent {
                step: step.to_string(),
                state: StepState::Failed,
                message: err.to_string(),
            }));
            Err(err)
        }
    }
}
