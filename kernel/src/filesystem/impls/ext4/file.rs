use alloc::string::String;
use core::any::Any;

use ext4plus::{Ext4, FollowSymlinks, file::File as Ext4InnerFile, inode::Inode, path::Path};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{LookupCache, chmod_path, lookup_cache_insert_raw},
    info::{FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{File, FileLikeType, Whence},
};

pub struct Ext4File {
    name: String,
    path: String,
    fs: Ext4,
    inner: Ext4InnerFile,
    parent_inode: u32,
    lookup_cache: LookupCache,
}

impl Ext4File {
    pub fn new(
        name: String,
        path: String,
        fs: Ext4,
        inner: Ext4InnerFile,
        parent_inode: u32,
        lookup_cache: LookupCache,
    ) -> Self {
        Self {
            name,
            path,
            fs,
            inner,
            parent_inode,
            lookup_cache,
        }
    }

    fn size(&self) -> Result<usize, FSError> {
        let meta = self.inner.inode().metadata();
        Ok(usize::try_from(meta.len()).unwrap())
    }

    pub fn inode(&self) -> Inode {
        self.inner.inode().clone()
    }

    fn update_lookup_cache(&self) {
        lookup_cache_insert_raw(
            &self.lookup_cache,
            self.parent_inode,
            &self.name,
            self.inner.inode(),
        );
    }
}

impl File for Ext4File {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        self.inner.read_bytes(buffer).map_err(Into::into)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let written = self.inner.write_bytes(buffer).map_err(FSError::from)?;
        self.update_lookup_cache();
        Ok(written)
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        self.inner.read_bytes_at(buffer, offset).map_err(Into::into)
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        let size = self.size()?;
        Ok(FileLikeInfo::new(
            self.name.clone(),
            size,
            UnixPermission(self.inner.inode().mode().bits() as u32),
            FileLikeType::File,
        )
        .with_inode(self.inner.inode().index.get().into()))
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let pos = match seek_type {
            Whence::Start => offset,
            Whence::Current => self.inner.position() as i64 + offset,
            Whence::End => self.inner.inode().size_in_bytes() as i64 + offset,
        };

        let _ = self.inner.seek_to(pos as u64);

        Ok(self.inner.position() as usize)
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        chmod_path(&self.fs, &self.path, mode)?;
        let inode = self
            .fs
            .path_to_inode(Path::new(&self.path), FollowSymlinks::All)
            .map_err(FSError::from)?;
        lookup_cache_insert_raw(&self.lookup_cache, self.parent_inode, &self.name, &inode);
        Ok(())
    }
}
