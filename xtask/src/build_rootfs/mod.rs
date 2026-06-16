mod apk;
mod command;
mod disk;
mod mount;

use anyhow::{Context, Result, bail};
use clap::Args;
use std::{env, fs, path::PathBuf};

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
    env::set_current_dir(&repo_root)
        .with_context(|| format!("failed to enter {}", repo_root.display()))?;

    let disk = repo_root.join("disk.img");
    let sysroot = repo_root.join("sysroot");
    fs::create_dir_all(&sysroot)
        .with_context(|| format!("failed to create {}", sysroot.display()))?;

    unmount_if_mounted(&sysroot)?;
    prepare_disk(&disk, args.override_disk()?)?;
    mount_disk(&disk, &sysroot)?;

    let _mount = MountedSysroot { path: &sysroot };
    write_repositories(&repo_root, &sysroot)?;
    install_packages(&sysroot)?;

    Ok(0)
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
