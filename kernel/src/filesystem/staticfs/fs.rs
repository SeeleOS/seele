use alloc::sync::Arc;
use spin::Mutex;

use crate::filesystem::{
    errors::FSError,
    path::{Path, PathPart},
    staticfs::{
        device::StaticDeviceHandle, directory::StaticDirectoryHandle, file::StaticFileHandle,
        node::StaticNode, symlink::StaticSymlinkHandle,
    },
    vfs::FSResult,
    vfs_traits::{FileLike, FileSystem},
};

pub struct StaticFs {
    root: &'static StaticNode,
}

impl StaticFs {
    pub fn new(root: &'static StaticNode) -> Self {
        Self { root }
    }

    fn materialize(&self, node: &'static StaticNode) -> FileLike {
        match node {
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
        }
    }
}

impl FileSystem for StaticFs {
    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let normalized = path.normalize();
        let mut current = self.root;

        for component in normalized.parts.iter() {
            match component {
                PathPart::Root | PathPart::CurrentDir => {}
                PathPart::ParentDir => return Err(FSError::NotADirectory),
                PathPart::Normal(name) => {
                    let directory = match current {
                        StaticNode::Directory(directory) => directory,
                        _ => return Err(FSError::NotADirectory),
                    };
                    let entry = directory
                        .entries
                        .iter()
                        .find(|entry| entry.name == name)
                        .ok_or(FSError::NotFound)?;
                    current = entry.node;
                }
            }
        }

        Ok(self.materialize(current))
    }
}
