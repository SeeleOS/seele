use anyhow::{Context, Result};
use std::{fs, path::Path};
use xshell::{Shell, cmd};

const DISK_SIZE: &str = "10G";

pub fn prepare_disk(sh: &Shell, disk: &Path, override_disk: bool) -> Result<()> {
    if disk.exists() && !override_disk {
        println!("reusing existing disk image: {}", disk.display());
        return Ok(());
    }

    if disk.exists() {
        fs::remove_file(disk).with_context(|| format!("failed to remove {}", disk.display()))?;
    }

    cmd!(sh, "truncate -s {DISK_SIZE} {disk}").run()?;
    cmd!(sh, "mkfs.ext4 -F {disk}").run()?;
    Ok(())
}
