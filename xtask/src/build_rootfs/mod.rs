mod arch;
mod mount;
mod rootfs_image;

use anyhow::{Context, Result, bail};
use clap::Args;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use xshell::Shell;

use crate::json_output::{JsonEvent, OutputMode, emit};

use self::{
    arch::{install_packages, set_empty_root_password},
    mount::{MountedRootfs, mount_rootfs_image, unmount_if_mounted},
    rootfs_image::prepare_rootfs_image,
};

#[derive(Debug, Args)]
pub struct BuildRootfsArgs {
    #[arg(long)]
    pub override_rootfs: bool,

    #[arg(long)]
    pub json_output: bool,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub passthrough: Vec<String>,
}

pub fn build_rootfs(args: BuildRootfsArgs) -> Result<i32> {
    let output_mode = args.output_mode();
    if output_mode.is_json() {
        emit(&JsonEvent::started("build-rootfs"))?;
    }

    let repo_root = repo_root()?;
    let mut sh = Shell::new()?;
    sh.set_current_dir(&repo_root);

    let target = target_dir(&repo_root);
    let rootfs_image = target.join("rootfs.img");
    let rootfs_mount = target.join("rootfs_mnt");
    if output_mode.is_json() {
        emit(&JsonEvent::progress(
            "build-rootfs",
            "paths",
            "resolved rootfs image and mount point",
        ))?;
    }
    fs::create_dir_all(&rootfs_mount)
        .with_context(|| format!("failed to create {}", rootfs_mount.display()))?;

    if output_mode.is_json() {
        emit(&JsonEvent::progress(
            "build-rootfs",
            "mount",
            "ensuring rootfs mount point is not mounted",
        ))?;
    }
    unmount_if_mounted(&sh, &rootfs_mount, output_mode)?;
    prepare_rootfs_image(&sh, &rootfs_image, args.override_rootfs()?, output_mode)?;
    if output_mode.is_json() {
        emit(&JsonEvent::progress(
            "build-rootfs",
            "mount",
            "mounting rootfs image",
        ))?;
    }
    mount_rootfs_image(&sh, &rootfs_image, &rootfs_mount, output_mode)?;

    let _mount = MountedRootfs {
        path: &rootfs_mount,
    };
    if output_mode.is_json() {
        emit(&JsonEvent::progress(
            "build-rootfs",
            "arch",
            "installing rootfs packages",
        ))?;
    }
    install_packages(&sh, &repo_root, &rootfs_mount, output_mode)?;
    if output_mode.is_json() {
        emit(&JsonEvent::progress(
            "build-rootfs",
            "arch",
            "setting empty root password",
        ))?;
    }
    set_empty_root_password(&sh, &rootfs_mount, output_mode)?;
    if output_mode.is_json() {
        emit(&JsonEvent::progress(
            "build-rootfs",
            "mount-points",
            "creating rootfs mount points",
        ))?;
    }
    create_mount_points(&rootfs_mount, output_mode)?;

    if output_mode.is_json() {
        emit(&JsonEvent::finished("build-rootfs", 0, "ok"))?;
    }
    Ok(0)
}

fn create_mount_points(rootfs_mount: &Path, output_mode: OutputMode) -> Result<()> {
    let sh = Shell::new()?;
    for path in [
        "dev",
        "dev/pts",
        "dev/shm",
        "proc",
        "run",
        "sys",
        "sys/fs",
        "sys/fs/cgroup",
        "tmp",
        "var/log",
        "var/tmp",
    ] {
        let full_path = rootfs_mount.join(path);
        crate::json_output::run_xshell_command(
            "build-rootfs",
            &sh,
            xshell::cmd!(sh, "sudo mkdir -p {full_path}"),
            output_mode,
        )
        .with_context(|| format!("failed to create rootfs mount point {path}"))?;
    }
    Ok(())
}

impl BuildRootfsArgs {
    fn output_mode(&self) -> OutputMode {
        if self.json_output {
            OutputMode::Json
        } else {
            OutputMode::Human
        }
    }

    fn override_rootfs(&self) -> Result<bool> {
        let mut override_rootfs = self.override_rootfs;

        for arg in &self.passthrough {
            match arg.as_str() {
                "--override" | "--override-rootfs" => override_rootfs = true,
                _ => bail!("unknown argument: {arg}"),
            }
        }

        Ok(override_rootfs)
    }
}

fn repo_root() -> Result<PathBuf> {
    Ok(env::current_dir()?)
}

fn target_dir(repo_root: &Path) -> PathBuf {
    repo_root.join("target")
}
