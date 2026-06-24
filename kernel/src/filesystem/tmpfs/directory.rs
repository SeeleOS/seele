use core::any::Any;

use alloc::{string::String, vec::Vec};

use crate::filesystem::{
    errors::FSError,
    info::{DirectoryContentInfo, FileLikeInfo, FileTimes, UnixPermission},
    vfs::FSResult,
    vfs_traits::{Directory, DirectoryContentType, FileLike, FileLikeType},
};

use super::{
    DEFAULT_FILE_MODE, TmpNodeKind, TmpfsState, TmpfsStateRef, node_name, tmpfs_lookup_path,
};

pub(crate) struct TmpfsDirectoryHandle {
    state: TmpfsStateRef,
    path: String,
}

impl TmpfsDirectoryHandle {
    pub(crate) fn new(state: TmpfsStateRef, path: String) -> Self {
        Self { state, path }
    }
}

impl Directory for TmpfsDirectoryHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        let state = self.state.lock();
        let node = state.node(&self.path)?;
        let mode = match &node.kind {
            TmpNodeKind::Directory { mode, .. } => *mode,
            TmpNodeKind::File { .. } | TmpNodeKind::Symlink { .. } => {
                return Err(FSError::NotADirectory);
            }
        };
        Ok(FileLikeInfo::new(
            node_name(&self.path),
            0,
            UnixPermission(mode),
            FileLikeType::Directory,
        )
        .with_inode(node.inode)
        .with_owner(node.uid, node.gid)
        .with_nlink(node.link_count)
        .with_times(node.times))
    }

    fn name(&self) -> FSResult<String> {
        Ok(node_name(&self.path))
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        let state = self.state.lock();
        let node = state.node(&self.path)?;
        let children = match &node.kind {
            TmpNodeKind::Directory { children, .. } => children,
            TmpNodeKind::File { .. } | TmpNodeKind::Symlink { .. } => {
                return Err(FSError::NotADirectory);
            }
        };

        let mut entries = Vec::new();
        for child in children {
            let child_path = TmpfsState::child_path(&self.path, child);
            let child_node = state.node(&child_path)?;
            let content_type = match child_node.kind {
                TmpNodeKind::Directory { .. } => DirectoryContentType::Directory,
                TmpNodeKind::File { .. } => DirectoryContentType::File,
                TmpNodeKind::Symlink { .. } => DirectoryContentType::Symlink,
            };
            entries.push(
                DirectoryContentInfo::new(child.clone(), content_type).with_inode(child_node.inode),
            );
        }
        Ok(entries)
    }

    fn create(&self, info: DirectoryContentInfo) -> FSResult<()> {
        let mut state = self.state.lock();
        match info.content_type {
            DirectoryContentType::File => state.create_file(
                &self.path,
                &info.name,
                info.permission
                    .unwrap_or(UnixPermission(DEFAULT_FILE_MODE))
                    .0,
                info.rdev,
            ),
            DirectoryContentType::Directory => state.create_directory(
                &self.path,
                &info.name,
                info.permission.unwrap_or(UnixPermission::directory()).0,
            ),
            DirectoryContentType::Symlink => Err(FSError::Readonly),
        }
    }

    fn create_symlink(&self, name: &str, target: &str) -> FSResult<()> {
        self.state.lock().create_symlink(&self.path, name, target)
    }

    fn delete(&self, name: &str) -> FSResult<()> {
        self.state.lock().delete_node(&self.path, name)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        let child_path = TmpfsState::child_path(&self.path, name);
        tmpfs_lookup_path(&self.state, &child_path)
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let mut state = self.state.lock();
        let node = state.node_mut(&self.path)?;
        match &mut node.kind {
            TmpNodeKind::Directory { mode: dir_mode, .. } => {
                *dir_mode = mode & 0o7777;
                Ok(())
            }
            TmpNodeKind::File { .. } | TmpNodeKind::Symlink { .. } => Err(FSError::NotADirectory),
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
