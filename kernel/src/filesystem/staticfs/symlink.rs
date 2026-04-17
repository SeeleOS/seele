use crate::filesystem::{
    info::{FileLikeInfo, UnixPermission},
    path::Path,
    staticfs::StaticSymlinkNode,
    vfs::FSResult,
    vfs_traits::{FileLikeType, Symlink},
};

pub struct StaticSymlinkHandle {
    node: &'static StaticSymlinkNode,
}

impl StaticSymlinkHandle {
    pub fn new(node: &'static StaticSymlinkNode) -> Self {
        Self { node }
    }
}

impl Symlink for StaticSymlinkHandle {
    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.node.name.into(),
            self.node.target.len(),
            UnixPermission(self.node.mode),
            FileLikeType::Symlink,
        )
        .with_inode(self.node.inode))
    }

    fn target(&self) -> FSResult<Path> {
        Ok(Path::new(self.node.target))
    }
}
