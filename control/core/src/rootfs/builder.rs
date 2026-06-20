use super::{
    config::BuildRootfsConfig,
    mount::{ensure_mounted_with_runner, unmount_with_runner},
    paths::paths,
    steps::run_step,
};
use crate::{
    Artifact, ArtifactKind, Event, JobContext, RootfsEvent, StepState, process::ProcessRunner,
};
use anyhow::{Context, Result, bail};
use std::{fs, path::Path, process::Command};

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
