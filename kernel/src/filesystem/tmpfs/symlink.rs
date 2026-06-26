use alloc::{string::String, vec::Vec};

use crate::filesystem::{
    errors::FSError,
    info::{FileLikeInfo, FileTimes, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{FileLikeType, Symlink},
};

use super::{TmpNodeKind, TmpfsStateRef, node_name};

pub(crate) struct TmpfsSymlinkHandle {
    state: TmpfsStateRef,
    path: String,
}

impl TmpfsSymlinkHandle {
    pub(crate) fn new(state: TmpfsStateRef, path: String) -> Self {
        Self { state, path }
    }
}

impl Symlink for TmpfsSymlinkHandle {
    fn info(&self) -> FSResult<FileLikeInfo> {
        let state = self.state.lock();
        let node = state.node(&self.path)?;
        match &node.kind {
            TmpNodeKind::Symlink { target } => Ok(FileLikeInfo::new(
                node_name(&self.path),
                target.len(),
                UnixPermission::symlink(),
                FileLikeType::Symlink,
            )
            .with_inode(node.inode)
            .with_owner(node.uid, node.gid)
            .with_nlink(node.link_count)
            .with_times(node.times)),
            TmpNodeKind::Directory { .. }
            | TmpNodeKind::File { .. }
            | TmpNodeKind::Device { .. } => Err(FSError::NotASymlink),
        }
    }

    fn target(&self) -> FSResult<Path> {
        let state = self.state.lock();
        let node = state.node(&self.path)?;
        match &node.kind {
            TmpNodeKind::Symlink { target } => Ok(Path::new(target)),
            TmpNodeKind::Directory { .. }
            | TmpNodeKind::File { .. }
            | TmpNodeKind::Device { .. } => Err(FSError::NotASymlink),
        }
    }

    fn read_link_target(&self) -> FSResult<String> {
        let state = self.state.lock();
        let node = state.node(&self.path)?;
        match &node.kind {
            TmpNodeKind::Symlink { target } => Ok(target.clone()),
            TmpNodeKind::Directory { .. }
            | TmpNodeKind::File { .. }
            | TmpNodeKind::Device { .. } => Err(FSError::NotASymlink),
        }
    }

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        let mut state = self.state.lock();
        let inode = state.node(&self.path)?.inode;
        state.update_owner_by_inode(inode, uid, gid)
    }

    fn set_times(&self, times: FileTimes) -> FSResult<()> {
        let mut state = self.state.lock();
        let inode = state.node(&self.path)?.inode;
        state.update_times_by_inode(inode, times)
    }

    fn get_xattr(&self, name: &str) -> FSResult<Option<Vec<u8>>> {
        let state = self.state.lock();
        let inode = state.node(&self.path)?.inode;
        Ok(state.xattr(inode, name))
    }

    fn set_xattr(&self, name: String, value: Vec<u8>, create: bool, replace: bool) -> FSResult<()> {
        let mut state = self.state.lock();
        let inode = state.node(&self.path)?.inode;
        state.set_xattr(inode, name, value, create, replace)
    }

    fn list_xattrs(&self) -> FSResult<Vec<String>> {
        let state = self.state.lock();
        let inode = state.node(&self.path)?.inode;
        state.list_xattrs(inode)
    }

    fn remove_xattr(&self, name: &str) -> FSResult<()> {
        let mut state = self.state.lock();
        let inode = state.node(&self.path)?.inode;
        state.remove_xattr(inode, name)
    }
}
