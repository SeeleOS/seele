use core::any::Any;

use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{
    errors::FSError,
    info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{Directory, FileLike, FileLikeType},
};

use super::{
    connection::{FuseConnection, attr_file_type},
    file::FuseFile,
    symlink::FuseSymlink,
};

pub struct FuseDirectory {
    pub connection: Arc<FuseConnection>,
    pub nodeid: u64,
}

impl FuseDirectory {
    pub fn new(connection: Arc<FuseConnection>, nodeid: u64) -> Self {
        Self { connection, nodeid }
    }
}

impl Directory for FuseDirectory {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        let attr = self.connection.getattr(self.nodeid)?;
        Ok(FileLikeInfo::new(
            "/".into(),
            attr.size as usize,
            UnixPermission(attr.mode),
            FileLikeType::Directory,
        )
        .with_owner(attr.uid, attr.gid)
        .with_inode(attr.ino))
    }

    fn name(&self) -> FSResult<String> {
        Ok("/".into())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        Ok(self
            .connection
            .read_dir(self.nodeid)?
            .into_iter()
            .map(|entry| entry.info)
            .collect())
    }

    fn create(&self, _info: DirectoryContentInfo) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn delete(&self, _name: &str) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        if name == "." {
            return Ok(FileLike::Directory(Arc::new(Mutex::new(Self::new(
                self.connection.clone(),
                self.nodeid,
            )))));
        }

        let entry = self.connection.lookup(self.nodeid, name)?;
        Ok(match attr_file_type(entry.attr) {
            FileLikeType::Directory => FileLike::Directory(Arc::new(Mutex::new(Self::new(
                self.connection.clone(),
                entry.nodeid,
            )))),
            FileLikeType::File => FileLike::File(Arc::new(Mutex::new(FuseFile::new(
                self.connection.clone(),
                entry.nodeid,
            )))),
            FileLikeType::Symlink => FileLike::Symlink(Arc::new(Mutex::new(FuseSymlink::new(
                self.connection.clone(),
                entry.nodeid,
            )))),
        })
    }
}
