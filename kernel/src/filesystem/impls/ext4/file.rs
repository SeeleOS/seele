use alloc::string::String;
use core::any::Any;

use ext4plus::{
    Ext4,
    FollowSymlinks,
    file::File as Ext4InnerFile,
    inode::{Inode, InodeMode},
    path::Path,
};

use crate::filesystem::{
    errors::FSError,
    info::{FileLikeInfo, UnixPermission},
    vfs_traits::{File, FileLikeType, Whence},
};

pub struct Ext4File {
    name: String,
    path: String,
    fs: Ext4,
    inner: Ext4InnerFile,
}

impl Ext4File {
    const FILE_TYPE_BITS: u16 = 0o170000;

    pub fn new(name: String, path: String, fs: Ext4, inner: Ext4InnerFile) -> Self {
        Self {
            name,
            path,
            fs,
            inner,
        }
    }

    fn size(&self) -> Result<usize, FSError> {
        let meta = self.inner.inode().metadata();
        Ok(usize::try_from(meta.len()).unwrap())
    }

    pub fn inode(&self) -> Inode {
        self.inner.inode().clone()
    }

    pub fn chmod(&self, mode: InodeMode) -> Result<(), FSError> {
        let mut inode = self
            .fs
            .path_to_inode(Path::new(self.path.as_str()), FollowSymlinks::All)
            .map_err(FSError::from)?;
        let merged_mode =
            (inode.mode().bits() & Self::FILE_TYPE_BITS) | (mode.bits() & !Self::FILE_TYPE_BITS);
        let merged_mode = InodeMode::from_bits(merged_mode).ok_or(FSError::Other)?;
        inode.set_mode(merged_mode).map_err(FSError::from)?;
        inode.write(&self.fs).map_err(FSError::from)?;
        Ok(())
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
            UnixPermission(self.inner.inode().mode().bits() as u32),
            FileLikeType::File,
        )
        .with_inode(self.inner.inode().index.get().into()))
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> crate::filesystem::vfs::FSResult<usize> {
        let pos = match seek_type {
            Whence::Start => offset,
            Whence::Current => self.inner.position() as i64 + offset,
            Whence::End => self.inner.inode().size_in_bytes() as i64 + offset,
        };

        self.inner.seek_to(pos as u64);

        Ok(self.inner.position() as usize)
    }
}
