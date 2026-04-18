use alloc::string::String;

use crate::filesystem::{
    info::{FileLikeInfo, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{FileLikeType, Symlink},
};

pub(super) struct ProcSymlink {
    name: String,
    inode: u64,
    target: String,
}

impl ProcSymlink {
    pub(super) fn new(name: String, inode: u64, target: String) -> Self {
        Self {
            name,
            inode,
            target,
        }
    }
}

impl Symlink for ProcSymlink {
    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.name.clone(),
            self.target.len(),
            UnixPermission::symlink(),
            FileLikeType::Symlink,
        )
        .with_inode(self.inode))
    }

    fn target(&self) -> FSResult<Path> {
        Ok(Path::new(&self.target))
    }
}
