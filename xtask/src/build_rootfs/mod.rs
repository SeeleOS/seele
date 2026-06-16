mod apk;
mod disk;
mod mount;

use anyhow::{Context, Result, bail};
use clap::Args;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use xshell::Shell;

use self::{
    apk::{install_packages, write_repositories},
    disk::prepare_disk,
    mount::{MountedSysroot, mount_disk, unmount_if_mounted},
};

#[derive(Debug, Args)]
pub struct BuildRootfsArgs {
    #[arg(long)]
    pub override_disk: bool,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub passthrough: Vec<String>,
}

pub fn build_rootfs(args: BuildRootfsArgs) -> Result<i32> {
    let repo_root = repo_root()?;
    let mut sh = Shell::new()?;
    sh.set_current_dir(&repo_root);

    let disk = repo_root.join("disk.img");
    let sysroot = repo_root.join("sysroot");
    fs::create_dir_all(&sysroot)
        .with_context(|| format!("failed to create {}", sysroot.display()))?;

    unmount_if_mounted(&sh, &sysroot)?;
    prepare_disk(&sh, &disk, args.override_disk()?)?;
    mount_disk(&sh, &disk, &sysroot)?;

    let _mount = MountedSysroot { path: &sysroot };
    write_repositories(&sh, &repo_root, &sysroot)?;
    install_packages(&sh, &sysroot)?;
    create_mount_points(&sysroot)?;

    Ok(0)
}

fn create_mount_points(sysroot: &Path) -> Result<()> {
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
        fs::create_dir_all(sysroot.join(path))
            .with_context(|| format!("failed to create rootfs mount point {path}"))?;
    }
    Ok(())
}

impl BuildRootfsArgs {
    fn override_disk(&self) -> Result<bool> {
        let mut override_disk = self.override_disk;

        for arg in &self.passthrough {
            match arg.as_str() {
                "--override" => override_disk = true,
                _ => bail!("unknown argument: {arg}"),
            }
        }

        Ok(override_disk)
    }
}

fn repo_root() -> Result<PathBuf> {
    Ok(env::current_dir()?)
}
