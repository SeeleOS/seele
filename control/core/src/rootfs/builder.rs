use super::{
    arch::{PacmanConfig, configure_login_services, install_packages, set_empty_root_password},
    aur::{install_aur_packages, validate_rebuild_packages},
    config::BuildRootfsConfig,
    kirk::install_kirk,
    mount::{ensure_mounted_with_runner, unmount_with_runner},
    paths::paths,
    steps::run_step,
};
use crate::{Artifact, ArtifactKind, ControlContext, process::ProcessRunner};
use anyhow::{Context, Result};
use std::{fs, path::Path, process::Command};

pub fn build_rootfs(
    repo: &Path,
    config: &BuildRootfsConfig,
    context: &dyn ControlContext,
) -> Result<i32> {
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
    let pacman_conf = PacmanConfig::create(repo)?;
    run_step(context, "install_base", || {
        install_packages(&runner, context, pacman_conf.path(), &paths.mount)
    })?;
    run_step(context, "set_empty_root_password", || {
        set_empty_root_password(&runner, context, &paths.mount)
    })?;
    run_step(context, "configure_login_services", || {
        configure_login_services(&runner, context, &paths.mount)
    })?;
    run_step(context, "install_aur", || {
        let rebuild_aur = config.rebuild_aur();
        validate_rebuild_packages(&rebuild_aur.packages)?;
        install_aur_packages(
            repo,
            &runner,
            context,
            pacman_conf.path(),
            &paths.mount,
            &rebuild_aur,
        )
    })?;
    run_step(context, "install_kirk_ltp", || {
        install_kirk(repo, &runner, context, &paths.mount)
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
