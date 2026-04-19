use alloc::{format, string::String};

use crate::filesystem::{
    errors::FSError,
    info::{FileLikeInfo, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{FileLikeType, Symlink},
};

use super::{TMPFS_STATE, TmpNodeKind, node_name};

pub(crate) struct TmpfsSymlinkHandle {
    path: String,
}

impl TmpfsSymlinkHandle {
    pub(crate) fn new(path: String) -> Self {
        Self { path }
    }
}

impl Symlink for TmpfsSymlinkHandle {
    fn info(&self) -> FSResult<FileLikeInfo> {
        let state = TMPFS_STATE.lock();
        let node = state.node(&self.path)?;
        match &node.kind {
            TmpNodeKind::Symlink { target } => Ok(FileLikeInfo::new(
                node_name(&self.path),
                target.len(),
                UnixPermission::symlink(),
                FileLikeType::Symlink,
            )
            .with_inode(node.inode)),
            TmpNodeKind::Directory { .. } | TmpNodeKind::File { .. } => Err(FSError::NotASymlink),
        }
    }

    fn target(&self) -> FSResult<Path> {
        let state = TMPFS_STATE.lock();
        let node = state.node(&self.path)?;
        let target = match &node.kind {
            TmpNodeKind::Symlink { target } => target.clone(),
            TmpNodeKind::Directory { .. } | TmpNodeKind::File { .. } => {
                return Err(FSError::NotASymlink);
            }
        };
        let parent = Path::new(&self.path).parent().unwrap_or_default();
        let combined = if target.starts_with('/') {
            target
        } else if parent.clone().as_string() == "/" {
            format!("/{}", target)
        } else {
            format!("{}/{}", parent.as_string(), target)
        };
        Ok(Path::new(&combined).normalize())
    }

    fn read_link_target(&self) -> FSResult<String> {
        let state = TMPFS_STATE.lock();
        let node = state.node(&self.path)?;
        match &node.kind {
            TmpNodeKind::Symlink { target } => Ok(target.clone()),
            TmpNodeKind::Directory { .. } | TmpNodeKind::File { .. } => Err(FSError::NotASymlink),
        }
    }
}
