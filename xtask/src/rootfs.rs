use crate::cli::repo_root;
use anyhow::{Context, Result, bail};
use std::process::Command;

pub fn build(override_disk: bool) -> Result<i32> {
    let repo_root = repo_root()?;
    let script = repo_root.join("rootfs_making/make_rootfs.sh");
    let mut command = Command::new(&script);
    command.current_dir(repo_root);
    if override_disk {
        command.arg("--override");
    }

    let status = command
        .status()
        .with_context(|| format!("failed to run {}", script.display()))?;
    if !status.success() {
        bail!("rootfs build failed with status {}", status);
    }
    Ok(0)
}
