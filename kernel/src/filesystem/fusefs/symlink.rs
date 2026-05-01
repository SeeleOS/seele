use alloc::{string::String, sync::Arc};
use crate::filesystem::{
    info::{FileLikeInfo, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{FileLikeType, Symlink},
};

use super::connection::FuseConnection;

pub struct FuseSymlink {
    connection: Arc<FuseConnection>,
    nodeid: u64,
}

impl FuseSymlink {
    pub fn new(connection: Arc<FuseConnection>, nodeid: u64) -> Self {
        Self { connection, nodeid }
    }
}

impl Symlink for FuseSymlink {
    fn info(&self) -> FSResult<FileLikeInfo> {
        let attr = self.connection.getattr(self.nodeid)?;
        Ok(FileLikeInfo::new(
            String::new(),
            attr.size as usize,
            UnixPermission(attr.mode),
            FileLikeType::Symlink,
        )
        .with_owner(attr.uid, attr.gid)
        .with_inode(attr.ino))
    }

    fn target(&self) -> FSResult<Path> {
        let target = self.connection.readlink(self.nodeid)?;
        Ok(Path::new(&target))
    }
}
