use anyhow::Result;
use std::path::Path;
use xshell::{Shell, cmd};

pub fn mount_disk(sh: &Shell, disk: &Path, sysroot: &Path) -> Result<()> {
    cmd!(sh, "sudo mount -o loop {disk} {sysroot}").run()?;
    Ok(())
}

pub fn unmount_if_mounted(sh: &Shell, sysroot: &Path) -> Result<()> {
    if is_mountpoint(sh, sysroot)? {
        cmd!(sh, "sudo umount -l {sysroot}").run()?;
    }
    Ok(())
}

fn is_mountpoint(sh: &Shell, path: &Path) -> Result<bool> {
    Ok(cmd!(sh, "mountpoint -q {path}").run().is_ok())
}

pub struct MountedSysroot<'a> {
    pub path: &'a Path,
}

impl Drop for MountedSysroot<'_> {
    fn drop(&mut self) {
        let path = self.path;
        if let Ok(sh) = Shell::new()
            && is_mountpoint(&sh, path).unwrap_or(false)
        {
            let _ = cmd!(sh, "sudo umount -l {path}").run();
        }
    }
}
