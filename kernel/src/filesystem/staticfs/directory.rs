use core::any::Any;

use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{
    errors::FSError,
    info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
    staticfs::{
        device::StaticDeviceHandle, file::StaticFileHandle, node::StaticNode,
        symlink::StaticSymlinkHandle, StaticDirectoryNode,
    },
    vfs::FSResult,
    vfs_traits::{Directory, FileLike, FileLikeType},
};

pub struct StaticDirectoryHandle {
    node: &'static StaticDirectoryNode,
}

impl StaticDirectoryHandle {
    pub fn new(node: &'static StaticDirectoryNode) -> Self {
        Self { node }
    }
}

impl Directory for StaticDirectoryHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.node.name.into(),
            0,
            UnixPermission(self.node.mode),
            FileLikeType::Directory,
        )
        .with_inode(self.node.inode))
    }

    fn name(&self) -> FSResult<String> {
        Ok(self.node.name.into())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        Ok(self
            .node
            .entries
            .iter()
            .map(|entry| DirectoryContentInfo::new(entry.name.into(), entry.node.content_type()))
            .collect())
    }

    fn create(&self, _info: DirectoryContentInfo) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn delete(&self, _name: &str) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        let entry = self
            .node
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or(FSError::NotFound)?;

        Ok(match entry.node {
            StaticNode::Directory(node) => {
                FileLike::Directory(Arc::new(Mutex::new(StaticDirectoryHandle::new(node))))
            }
            StaticNode::File(node) => {
                FileLike::File(Arc::new(Mutex::new(StaticFileHandle::new(node))))
            }
            StaticNode::Symlink(node) => {
                FileLike::Symlink(Arc::new(Mutex::new(StaticSymlinkHandle::new(node))))
            }
            StaticNode::Device(node) => {
                FileLike::File(Arc::new(Mutex::new(StaticDeviceHandle::new(node))))
            }
        })
    }
}
