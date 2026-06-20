mod arch;
mod aur;
mod kirk;
mod mount;
mod rootfs_image;

use anyhow::{Context, Result, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use xshell::Shell;

use crate::reporter::{
    FinishStatus, WorkflowReporter, finished, progress, run_xshell_command, started,
};

use self::{
    arch::{
        configure_login_services, create_pacman_config, install_packages, set_empty_root_password,
    },
    aur::{install_aur_packages, validate_rebuild_packages},
    kirk::install_kirk,
    mount::{MountedRootfs, mount_rootfs_image, unmount_if_mounted},
    rootfs_image::prepare_rootfs_image,
};

#[derive(Debug, Default, Clone)]
pub struct BuildRootfsConfig {
    pub override_rootfs: bool,
    pub rebuild_aur: bool,
    pub rebuild_aur_packages: Vec<String>,
    pub passthrough: Vec<String>,
}

pub fn build_rootfs(config: BuildRootfsConfig, reporter: &dyn WorkflowReporter) -> Result<i32> {
    started(reporter, "build-rootfs")?;
    let repo_root = repo_root()?;
    let mut sh = Shell::new()?;
    sh.set_current_dir(&repo_root);

    let target = target_dir(&repo_root);
    let rootfs_image = target.join("rootfs.img");
    let rootfs_mount = target.join("rootfs_mnt");
    progress(
        reporter,
        "build-rootfs",
        "paths",
        "resolved rootfs image and mount point",
    )?;
    fs::create_dir_all(&rootfs_mount)
        .with_context(|| format!("failed to create {}", rootfs_mount.display()))?;

    progress(
        reporter,
        "build-rootfs",
        "mount",
        "ensuring rootfs mount point is not mounted",
    )?;
    unmount_if_mounted(&sh, &rootfs_mount, reporter)?;
    prepare_rootfs_image(&sh, &rootfs_image, config.override_rootfs()?, reporter)?;
    progress(reporter, "build-rootfs", "mount", "mounting rootfs image")?;
    mount_rootfs_image(&sh, &rootfs_image, &rootfs_mount, reporter)?;

    let _mount = MountedRootfs {
        path: &rootfs_mount,
    };
    progress(
        reporter,
        "build-rootfs",
        "arch",
        "installing rootfs packages",
    )?;
    let pacman_conf = create_pacman_config(&repo_root)?;
    install_packages(&sh, pacman_conf.path(), &rootfs_mount, reporter)?;
    progress(
        reporter,
        "build-rootfs",
        "arch",
        "setting empty root password",
    )?;
    set_empty_root_password(&sh, &rootfs_mount, reporter)?;
    progress(
        reporter,
        "build-rootfs",
        "arch",
        "configuring login services",
    )?;
    configure_login_services(&sh, &rootfs_mount, reporter)?;
    progress(reporter, "build-rootfs", "arch", "installing AUR packages")?;
    install_aur_packages(
        &sh,
        &repo_root,
        pacman_conf.path(),
        &rootfs_mount,
        config.rebuild_aur()?,
        reporter,
    )?;
    progress(
        reporter,
        "build-rootfs",
        "kirk",
        "installing kirk test runner",
    )?;
    install_kirk(&sh, &repo_root, &rootfs_mount, reporter)?;
    progress(
        reporter,
        "build-rootfs",
        "arch",
        "configuring ext4 mkfs defaults",
    )?;
    configure_mke2fs(&rootfs_mount, reporter)?;
    progress(
        reporter,
        "build-rootfs",
        "mount-points",
        "creating rootfs mount points",
    )?;
    create_mount_points(&rootfs_mount, reporter)?;

    finished(reporter, "build-rootfs", 0, FinishStatus::Ok)?;
    Ok(0)
}

fn configure_mke2fs(rootfs_mount: &Path, reporter: &dyn WorkflowReporter) -> Result<()> {
    let sh = Shell::new()?;
    let mke2fs_config = rootfs_mount.join("etc").join("mke2fs.conf");
    run_xshell_command(
        "build-rootfs",
        &sh,
        xshell::cmd!(
            sh,
            "sudo sed -i '/^[[:space:]]*features = has_journal,/ { /filetype/! s/has_journal,/has_journal,filetype,/ }' {mke2fs_config}"
        ),
        reporter,
    )
    .context("failed to configure mke2fs ext4 defaults")
}

fn create_mount_points(rootfs_mount: &Path, reporter: &dyn WorkflowReporter) -> Result<()> {
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
        run_xshell_command(
            "build-rootfs",
            &sh,
            xshell::cmd!(sh, "sudo mkdir -p {full_path}"),
            reporter,
        )
        .with_context(|| format!("failed to create rootfs mount point {path}"))?;
    }
    Ok(())
}

impl BuildRootfsConfig {
    fn override_rootfs(&self) -> Result<bool> {
        let mut override_rootfs = self.override_rootfs;

        let mut passthrough = self.passthrough.iter();
        while let Some(arg) = passthrough.next() {
            match arg.as_str() {
                "--override" | "--override-rootfs" => override_rootfs = true,
                "--rebuild-aur" => {}
                "--rebuild-aur-package" => {
                    if passthrough.next().is_none() {
                        bail!("missing package name for --rebuild-aur-package");
                    }
                }
                _ => bail!("unknown argument: {arg}"),
            }
        }

        Ok(override_rootfs)
    }

    fn rebuild_aur(&self) -> Result<RebuildAur> {
        let mut rebuild_aur = self.rebuild_aur;
        let mut rebuild_aur_packages = self.rebuild_aur_packages.clone();

        let mut passthrough = self.passthrough.iter();
        while let Some(arg) = passthrough.next() {
            match arg.as_str() {
                "--rebuild-aur" => rebuild_aur = true,
                "--rebuild-aur-package" => {
                    let Some(package) = passthrough.next() else {
                        bail!("missing package name for --rebuild-aur-package");
                    };
                    rebuild_aur_packages.push(package.clone());
                }
                "--override" | "--override-rootfs" => {}
                _ => bail!("unknown argument: {arg}"),
            }
        }

        rebuild_aur_packages.sort();
        rebuild_aur_packages.dedup();
        validate_rebuild_packages(&rebuild_aur_packages)?;

        Ok(RebuildAur {
            all: rebuild_aur,
            packages: rebuild_aur_packages,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RebuildAur {
    pub all: bool,
    pub packages: Vec<String>,
}

fn repo_root() -> Result<PathBuf> {
    Ok(env::current_dir()?)
}

fn target_dir(repo_root: &Path) -> PathBuf {
    repo_root.join("target")
}
