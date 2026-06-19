use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};
use xshell::{Shell, cmd};

use crate::reporter::{WorkflowReporter, log_event, progress, run_xshell_command};

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
        check_rootfs_image(sh, image, reporter)?;
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
    check_rootfs_image(sh, image, reporter)?;
    Ok(())
}

fn check_rootfs_image(sh: &Shell, image: &Path, reporter: &dyn WorkflowReporter) -> Result<()> {
    let message = format!("checking rootfs image: {}", image.display());
    progress(reporter, "build-rootfs", "rootfs-image-fsck", &message)?;
    let mut command = Command::new("e2fsck");
    command.current_dir(sh.current_dir()).arg("-fy").arg(image);

    if !reporter.capture_subprocess_output() {
        let status = command.status().context("failed to run e2fsck")?;
        if e2fsck_repaired_or_clean(status.code()) {
            return Ok(());
        }
        bail!("e2fsck failed with status {status}");
    }

    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to run e2fsck")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        log_event(reporter, "build-rootfs", "stdout", &stdout)?;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        log_event(reporter, "build-rootfs", "stderr", &stderr)?;
    }

    if e2fsck_repaired_or_clean(output.status.code()) {
        return Ok(());
    }
    bail!("e2fsck failed with status {}", output.status)
}

fn e2fsck_repaired_or_clean(code: Option<i32>) -> bool {
    matches!(code, Some(0 | 1))
}
