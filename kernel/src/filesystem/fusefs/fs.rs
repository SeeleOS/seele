use crate::memory::utils::Mut;
use alloc::sync::Arc;

use crate::filesystem::{
    errors::FSError,
    path::{Path, PathPart},
    vfs::FSResult,
    vfs_traits::{FileLike, FileLikeType, FileSystem, MountFlags},
};

use super::{
    connection::{FuseConnection, attr_file_type},
    directory::FuseDirectory,
    file::FuseFile,
    symlink::FuseSymlink,
};

#[derive(Debug)]
pub struct FuseFs {
    connection: Arc<FuseConnection>,
}

impl FuseFs {
    pub fn new(connection: Arc<FuseConnection>) -> Self {
        Self { connection }
    }

    fn resolve_path(&self, path: &Path) -> Result<(u64, FileLikeType), FSError> {
        let mut nodeid = self.connection.root_id();
        let mut kind = FileLikeType::Directory;

        for part in path.normalize().parts {
            match part {
                PathPart::Root | PathPart::CurrentDir => {}
                PathPart::ParentDir => return Err(FSError::NotADirectory),
                PathPart::Normal(name) => {
                    let entry = self.connection.lookup(nodeid, &name)?;
                    nodeid = entry.nodeid;
                    kind = attr_file_type(entry.attr);
                }
            }
        }

        Ok((nodeid, kind))
    }
}

impl FileSystem for FuseFs {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn init(&mut self) -> FSResult<()> {
        self.connection.mount_ready()
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let (nodeid, kind) = self.resolve_path(path)?;
        Ok(match kind {
            FileLikeType::Directory => FileLike::Directory(Arc::new(Mut::new(FuseDirectory::new(
                self.connection.clone(),
                nodeid,
            )))),
            FileLikeType::File => FileLike::File(Arc::new(Mut::new(FuseFile::new(
                self.connection.clone(),
                nodeid,
            )))),
            FileLikeType::Symlink => FileLike::Symlink(Arc::new(Mut::new(FuseSymlink::new(
                self.connection.clone(),
                nodeid,
            )))),
        })
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
