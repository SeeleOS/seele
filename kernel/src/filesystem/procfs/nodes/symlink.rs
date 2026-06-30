use alloc::{string::String, sync::Arc};

use crate::filesystem::{
    info::{FileLikeInfo, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{FileLikeType, Symlink},
};

pub(super) struct ProcSymlink {
    name: String,
    inode: u64,
    target: ProcSymlinkTarget,
    magic_link: bool,
}

enum ProcSymlinkTarget {
    Static(String),
    Dynamic(Arc<dyn Fn() -> FSResult<String> + Send + Sync>),
}

impl ProcSymlink {
    pub(super) fn new(name: String, inode: u64, target: String) -> Self {
        Self {
            name,
            inode,
            target: ProcSymlinkTarget::Static(target),
            magic_link: false,
        }
    }

    pub(super) fn new_dynamic(
        name: String,
        inode: u64,
        target: Arc<dyn Fn() -> FSResult<String> + Send + Sync>,
    ) -> Self {
        Self {
            name,
            inode,
            target: ProcSymlinkTarget::Dynamic(target),
            magic_link: false,
        }
    }

    pub(super) fn new_magic_dynamic(
        name: String,
        inode: u64,
        target: Arc<dyn Fn() -> FSResult<String> + Send + Sync>,
    ) -> Self {
        Self {
            name,
            inode,
            target: ProcSymlinkTarget::Dynamic(target),
            magic_link: true,
        }
    }

    fn target_string(&self) -> FSResult<String> {
        match &self.target {
            ProcSymlinkTarget::Static(target) => Ok(target.clone()),
            ProcSymlinkTarget::Dynamic(target) => target(),
        }
    }
}

impl Symlink for ProcSymlink {
    fn info(&self) -> FSResult<FileLikeInfo> {
        let target = self.target_string()?;
        Ok(FileLikeInfo::new(
            self.name.clone(),
            target.len(),
            UnixPermission::symlink(),
            FileLikeType::Symlink,
        )
        .with_inode(self.inode))
    }

    fn target(&self) -> FSResult<Path> {
        Ok(Path::new(&self.target_string()?))
    }

    fn is_magic_link(&self) -> bool {
        self.magic_link
    }
}
