use anyhow::{Context, Result};
use std::{fs, path::Path};
use xshell::{Shell, cmd};

use crate::reporter::{WorkflowReporter, progress, run_xshell_command};

const DISK_SIZE: &str = "10G";

pub fn prepare_rootfs_image(
    sh: &Shell,
    image: &Path,
    override_rootfs: bool,
    reporter: &dyn WorkflowReporter,
) -> Result<()> {
    if image.exists() && !override_rootfs {
        let message = format!("reusing existing rootfs image: {}", image.display());
        progress(reporter, "build-rootfs", "rootfs-image", &message)?;
        return Ok(());
    }

    progress(
        reporter,
        "build-rootfs",
        "rootfs-image",
        "creating rootfs image",
    )?;
    if image.exists() {
        fs::remove_file(image).with_context(|| format!("failed to remove {}", image.display()))?;
    }

    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "truncate -s {DISK_SIZE} {image}"),
        reporter,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "mkfs.ext4 -F {image}"),
        reporter,
    )?;
    Ok(())
}
