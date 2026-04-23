use core::any::Any;

use alloc::{string::String, vec::Vec};

use crate::filesystem::{
    errors::FSError,
    info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{Directory, FileLike, FileLikeType},
};

pub(super) struct ProcDirectory {
    name: String,
    path: String,
    inode: u64,
    entries: Vec<DirectoryContentInfo>,
}

impl ProcDirectory {
    pub(super) fn new(
        name: String,
        path: String,
        inode: u64,
        entries: Vec<DirectoryContentInfo>,
    ) -> Self {
        Self {
            name,
            path,
            inode,
            entries,
        }
    }
}

impl Directory for ProcDirectory {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.name.clone(),
            0,
            UnixPermission(0o040555),
            FileLikeType::Directory,
        )
        .with_inode(self.inode))
    }

    fn name(&self) -> FSResult<String> {
        Ok(self.name.clone())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        Ok(self.entries.clone())
    }

    fn create(&self, _info: DirectoryContentInfo) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn delete(&self, _name: &str) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        let child_path = if self.path == "/" {
            alloc::format!("/{name}")
        } else {
            alloc::format!("{}/{}", self.path, name)
        };
        super::super::lookup_proc_path(&crate::filesystem::path::Path::new(&child_path))
    }
}
