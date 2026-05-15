use crate::cli::repo_root;
use anyhow::{Result, bail};
use std::process::Command;

pub fn mount() -> Result<i32> {
    let repo_root = repo_root()?;
    let sysroot = repo_root.join("sysroot");
    let disk = repo_root.join("disk.img");

    let status = Command::new("mountpoint")
        .arg("-q")
        .arg(&sysroot)
        .status()?;
    if status.success() {
        return Ok(0);
    }

    let status = Command::new("sudo")
        .arg("mount")
        .arg("-o")
        .arg("loop")
        .arg(&disk)
        .arg(&sysroot)
        .status()?;
    if !status.success() {
        bail!("failed to mount sysroot from {}", disk.display());
    }
    Ok(0)
}
