use anyhow::Result;
use std::path::Path;
use xshell::{Shell, cmd};

use crate::json_output::{OutputMode, run_xshell_command};

pub fn mount_rootfs_image(
    sh: &Shell,
    image: &Path,
    rootfs_mount: &Path,
    output_mode: OutputMode,
) -> Result<()> {
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo mount -o loop {image} {rootfs_mount}"),
        output_mode,
    )?;
    Ok(())
}

pub fn unmount_if_mounted(sh: &Shell, rootfs_mount: &Path, output_mode: OutputMode) -> Result<()> {
    if is_mountpoint(sh, rootfs_mount)? {
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "sudo umount -l {rootfs_mount}"),
            output_mode,
        )?;
    }
    Ok(())
}

fn is_mountpoint(sh: &Shell, path: &Path) -> Result<bool> {
    Ok(cmd!(sh, "mountpoint -q {path}").run().is_ok())
}

pub struct MountedRootfs<'a> {
    pub path: &'a Path,
}

impl Drop for MountedRootfs<'_> {
    fn drop(&mut self) {
        let path = self.path;
        if let Ok(sh) = Shell::new()
            && is_mountpoint(&sh, path).unwrap_or(false)
        {
            let _ = cmd!(sh, "sudo umount -l {path}").run();
        }
    }
}
