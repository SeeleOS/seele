use alloc::string::String;
use core::any::Any;

use ext4plus::{file::File as Ext4InnerFile, inode::Inode};
use seele_sys::abi::object::SeekType;

use crate::filesystem::{
    errors::FSError,
    info::{self, FileLikeInfo},
    vfs_traits::{File, FileLikeType},
};

pub struct Ext4File {
    name: String,
    inner: Ext4InnerFile,
}

impl Ext4File {
    pub fn new(name: String, inner: Ext4InnerFile) -> Self {
        Self { name, inner }
    }

    fn size(&self) -> Result<usize, FSError> {
        let meta = self.inner.inode().metadata();
        Ok(usize::try_from(meta.len()).unwrap())
    }

    pub fn inode(&self) -> Inode {
        self.inner.inode().clone()
    }
}

impl File for Ext4File {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn read(&mut self, buffer: &mut [u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.read_bytes(buffer).map_err(Into::into)
    }

    fn write(&mut self, buffer: &[u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.write_bytes(buffer).map_err(Into::into)
    }

    fn read_at(
        &mut self,
        buffer: &mut [u8],
        offset: u64,
    ) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.read_bytes_at(buffer, offset).map_err(Into::into)
    }

    fn info(&mut self) -> crate::filesystem::vfs::FSResult<FileLikeInfo> {
        let size = self.size()?;
        Ok(FileLikeInfo::new(
            self.name.clone(),
            size,
            FileLikeType::File,
        ))
    }

    fn seek(
        &mut self,
        offset: i64,
        seek_type: seele_sys::abi::object::SeekType,
    ) -> crate::filesystem::vfs::FSResult<usize> {
        let pos = match seek_type {
            SeekType::Start => offset,
            SeekType::Current => self.inner.position() as i64 + offset,
            SeekType::End => self.inner.inode().size_in_bytes() as i64 - offset,
        };

        self.inner.seek_to(pos as u64);

        Ok(self.inner.position() as usize)
    }
}
