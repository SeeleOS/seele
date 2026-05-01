use crate::filesystem::{
    errors::FSError,
    path::Path,
    vfs::FSResult,
    vfs_traits::{FileLike, FileSystem, MountFlags},
};

#[derive(Debug, Default)]
pub struct FuseFs;

impl FuseFs {
    pub fn new() -> Self {
        Self
    }
}

impl FileSystem for FuseFs {
    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, _path: &Path) -> FSResult<FileLike> {
        Err(FSError::NotFound)
    }

    fn rename(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn link(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn name(&self) -> &'static str {
        "fuse"
    }

    fn magic(&self) -> i64 {
        0x6573_5546
    }

    fn mount_source(&self) -> &'static str {
        "fuse"
    }

    fn default_mount_flags(&self, _path: &Path) -> MountFlags {
        MountFlags::MS_NOSUID | MountFlags::MS_NODEV | MountFlags::MS_RELATIME
    }
}
