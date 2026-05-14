use alloc::string::String;
use core::any::Any;

use ext4plus::{Ext4, file, inode::Inode};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{LookupCache, chmod_inode, lookup_cache_insert_raw},
    info::{FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{File, FileLikeType, Whence},
};

pub struct Ext4File {
    name: String,
    fs: Ext4,
    inode: Inode,
    position: u64,
    parent_inode: u32,
    lookup_cache: LookupCache,
}

impl Ext4File {
    pub fn new(
        name: String,
        fs: Ext4,
        inode: Inode,
        parent_inode: u32,
        lookup_cache: LookupCache,
    ) -> Self {
        Self {
            name,
            fs,
            inode,
            position: 0,
            parent_inode,
            lookup_cache,
        }
    }

    fn size(&self) -> Result<usize, FSError> {
        let meta = self.inode.metadata();
        Ok(usize::try_from(meta.len()).unwrap())
    }

    pub fn inode(&self) -> Inode {
        self.inode.clone()
    }

    fn update_lookup_cache(&self) {
        lookup_cache_insert_raw(
            &self.lookup_cache,
            self.parent_inode,
            &self.name,
            &self.inode,
        );
    }
}

impl File for Ext4File {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let read =
            file::read_at(&self.fs, &self.inode, buffer, self.position).map_err(FSError::from)?;
        self.position = self.position.saturating_add(read as u64);
        Ok(read)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let written = file::write_at(&self.fs, &mut self.inode, buffer, self.position)
            .map_err(FSError::from)?;
        self.position = self.position.saturating_add(written as u64);
        self.update_lookup_cache();
        Ok(written)
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        file::read_at(&self.fs, &self.inode, buffer, offset).map_err(Into::into)
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        let size = self.size()?;
        Ok(FileLikeInfo::new(
            self.name.clone(),
            size,
            UnixPermission(self.inode.mode().bits() as u32),
            FileLikeType::File,
        )
        .with_owner(self.inode.uid(), self.inode.gid())
        .with_inode(self.inode.index.get().into()))
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let len = self.inode.size_in_bytes() as i64;
        let pos = match seek_type {
            Whence::Start => offset,
            Whence::Current => self.position as i64 + offset,
            Whence::End => len + offset,
            Whence::Data => {
                if offset < 0 || offset >= len {
                    return Err(FSError::Other);
                }
                offset
            }
            Whence::Hole => {
                if offset < 0 || offset > len {
                    return Err(FSError::Other);
                }
                len
            }
        };

        self.position = pos as u64;

        Ok(self.position as usize)
    }

    fn truncate(&mut self, length: u64) -> FSResult<()> {
        file::truncate(&self.fs, &mut self.inode, length).map_err(FSError::from)?;
        self.position = self.position.min(length);
        self.update_lookup_cache();
        Ok(())
    }

    fn allocate(&mut self, mode: u32, offset: u64, len: u64) -> FSResult<()> {
        if mode != 0 {
            return Err(FSError::Other);
        }

        let end = offset.checked_add(len).ok_or(FSError::Other)?;
        if end > self.inode.size_in_bytes() {
            file::truncate(&self.fs, &mut self.inode, end).map_err(FSError::from)?;
            self.update_lookup_cache();
        }
        Ok(())
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let mut inode = self.inode.clone();
        chmod_inode(&self.fs, &mut inode, mode)?;
        lookup_cache_insert_raw(&self.lookup_cache, self.parent_inode, &self.name, &inode);
        Ok(())
    }
}
