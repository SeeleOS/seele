use crate::cli::repo_root;
use anyhow::Result;
use xshell::{Shell, cmd};

pub fn mount() -> Result<i32> {
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
