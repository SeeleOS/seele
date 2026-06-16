use anyhow::Result;
use clap::Args;
use std::{env, path::PathBuf};
use xshell::{Shell, cmd};

#[derive(Debug, Args)]
pub struct RootfsArgs {
    #[arg(long)]
    pub r#override: bool,
}

pub fn rootfs(args: RootfsArgs) -> Result<i32> {
    let sh = Shell::new()?;
    let repo_root = repo_root()?;
    let script = repo_root.join("rootfs_making/make_rootfs.sh");
    let mut sh = sh;
    sh.set_current_dir(repo_root);

    if args.r#override {
        cmd!(sh, "{script} --override").run()?;
    } else {
        cmd!(sh, "{script}").run()?;
    }

    Ok(0)
}

fn repo_root() -> Result<PathBuf> {
    Ok(env::current_dir()?)
}
