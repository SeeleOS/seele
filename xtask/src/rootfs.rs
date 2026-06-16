use crate::cli::repo_root;
use anyhow::Result;
use xshell::{Shell, cmd};

pub fn build(override_disk: bool) -> Result<i32> {
    let sh = Shell::new()?;
    let repo_root = repo_root()?;
    let script = repo_root.join("rootfs_making/make_rootfs.sh");
    let mut sh = sh;
    sh.set_current_dir(repo_root);

    if override_disk {
        cmd!(sh, "{script} --override").run()?;
    } else {
        cmd!(sh, "{script}").run()?;
    }

    Ok(0)
}
