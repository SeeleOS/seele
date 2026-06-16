use crate::build_rootfs::command::run;
use anyhow::{Context, Result};
use std::{fs, path::Path, process::Command};

const DISK_SIZE: &str = "10G";

pub fn prepare_disk(disk: &Path, override_disk: bool) -> Result<()> {
    if disk.exists() && !override_disk {
        println!("reusing existing disk image: {}", disk.display());
        return Ok(());
    }

    if disk.exists() {
        fs::remove_file(disk).with_context(|| format!("failed to remove {}", disk.display()))?;
    }

    run(Command::new("truncate").arg("-s").arg(DISK_SIZE).arg(disk))?;
    run(Command::new("mkfs.ext4").arg("-F").arg(disk))?;
    Ok(())
}
