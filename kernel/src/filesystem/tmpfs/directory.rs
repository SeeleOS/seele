use core::any::Any;

use alloc::{string::String, vec::Vec};

use crate::filesystem::{
    errors::FSError,
    info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{Directory, DirectoryContentType, FileLike, FileLikeType},
};

use super::{TMPFS_STATE, TmpNodeKind, TmpfsState, node_name, tmpfs_lookup_path};

pub(crate) struct TmpfsDirectoryHandle {
    path: String,
}

impl TmpfsDirectoryHandle {
    pub(crate) fn new(path: String) -> Self {
        Self { path }
    }
}

impl Directory for TmpfsDirectoryHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        let state = TMPFS_STATE.lock();
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
        .with_inode(node.inode))
    }

    fn name(&self) -> FSResult<String> {
        Ok(node_name(&self.path))
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        let state = TMPFS_STATE.lock();
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
            entries.push(DirectoryContentInfo::new(child.clone(), content_type));
        }
        Ok(entries)
    }

    fn create(&self, info: DirectoryContentInfo) -> FSResult<()> {
        let mut state = TMPFS_STATE.lock();
        match info.content_type {
            DirectoryContentType::File => state.create_file(&self.path, &info.name),
            DirectoryContentType::Directory => state.create_directory(&self.path, &info.name),
            DirectoryContentType::Symlink => Err(FSError::Readonly),
        }
    }

    fn create_symlink(&self, name: &str, target: &str) -> FSResult<()> {
        TMPFS_STATE.lock().create_symlink(&self.path, name, target)
    }

    fn delete(&self, name: &str) -> FSResult<()> {
        TMPFS_STATE.lock().delete_node(&self.path, name)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        let child_path = TmpfsState::child_path(&self.path, name);
        tmpfs_lookup_path(&child_path)
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let mut state = TMPFS_STATE.lock();
        let node = state.node_mut(&self.path)?;
        match &mut node.kind {
            TmpNodeKind::Directory { mode: dir_mode, .. } => {
                *dir_mode = mode & 0o7777;
                Ok(())
            }
            TmpNodeKind::File { .. } | TmpNodeKind::Symlink { .. } => Err(FSError::NotADirectory),
        }
    }
}
