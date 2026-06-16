use crate::build_rootfs::command::run;
use anyhow::{Context, Result};
use std::{path::Path, process::Command};

pub fn mount_disk(disk: &Path, sysroot: &Path) -> Result<()> {
    run(Command::new("sudo")
        .arg("mount")
        .arg("-o")
        .arg("loop")
        .arg(disk)
        .arg(sysroot))
}

pub fn unmount_if_mounted(sysroot: &Path) -> Result<()> {
    if is_mountpoint(sysroot)? {
        run(Command::new("sudo").arg("umount").arg("-l").arg(sysroot))?;
    }
    Ok(())
}

fn is_mountpoint(path: &Path) -> Result<bool> {
    let status = Command::new("mountpoint")
        .arg("-q")
        .arg(path)
        .status()
        .with_context(|| format!("failed to inspect mountpoint {}", path.display()))?;
    Ok(status.success())
}

pub struct MountedSysroot<'a> {
    pub path: &'a Path,
}

impl Drop for MountedSysroot<'_> {
    fn drop(&mut self) {
        if is_mountpoint(self.path).unwrap_or(false) {
            let _ = Command::new("sudo")
                .arg("umount")
                .arg("-l")
                .arg(self.path)
                .status();
        }
    }
}
