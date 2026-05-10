use core::any::Any;

use alloc::{string::String, sync::Arc, vec::Vec};

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
    entries_fn: Option<Arc<dyn Fn() -> Vec<DirectoryContentInfo> + Send + Sync>>,
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
            entries_fn: None,
        }
    }

    pub(super) fn new_dynamic(
        name: String,
        path: String,
        inode: u64,
        entries_fn: Arc<dyn Fn() -> Vec<DirectoryContentInfo> + Send + Sync>,
    ) -> Self {
        Self {
            name,
            path,
            inode,
            entries: Vec::new(),
            entries_fn: Some(entries_fn),
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
        if let Some(entries_fn) = &self.entries_fn {
            return Ok(entries_fn());
        }
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
