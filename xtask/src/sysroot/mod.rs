use anyhow::Result;
use clap::Args;
use std::{env, path::PathBuf};
use xshell::{Shell, cmd};

#[derive(Debug, Args)]
pub struct SysrootArgs {}

pub fn sysroot(_args: SysrootArgs) -> Result<i32> {
    let sh = Shell::new()?;
    let repo_root = repo_root()?;
    let sysroot = repo_root.join("sysroot");
    let disk = repo_root.join("disk.img");

    if cmd!(sh, "mountpoint -q {sysroot}")
        .ignore_status()
        .output()?
        .status
        .success()
    {
        return Ok(0);
    }

    cmd!(sh, "sudo mount -o loop {disk} {sysroot}").run()?;
    Ok(0)
}

fn repo_root() -> Result<PathBuf> {
    Ok(env::current_dir()?)
}
