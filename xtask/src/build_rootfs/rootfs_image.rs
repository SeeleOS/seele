use anyhow::{Context, Result};
use std::{fs, path::Path};
use xshell::{Shell, cmd};

use crate::json_output::{JsonEvent, OutputMode, emit, run_xshell_command};

const DISK_SIZE: &str = "10G";

pub fn prepare_rootfs_image(
    sh: &Shell,
    image: &Path,
    override_rootfs: bool,
    output_mode: OutputMode,
) -> Result<()> {
    if image.exists() && !override_rootfs {
        let message = format!("reusing existing rootfs image: {}", image.display());
        if output_mode.is_json() {
            emit(&JsonEvent::progress(
                "build-rootfs",
                "rootfs-image",
                &message,
            ))?;
        } else {
            println!("{message}");
        }
        return Ok(());
    }

    if output_mode.is_json() {
        emit(&JsonEvent::progress(
            "build-rootfs",
            "rootfs-image",
            "creating rootfs image",
        ))?;
    }
    if image.exists() {
        fs::remove_file(image).with_context(|| format!("failed to remove {}", image.display()))?;
    }

    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "truncate -s {DISK_SIZE} {image}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "mkfs.ext4 -F {image}"),
        output_mode,
    )?;
    Ok(())
}
