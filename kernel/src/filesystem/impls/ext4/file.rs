use alloc::string::String;

use ext4plus::file::File as Ext4InnerFile;

use crate::filesystem::{
    errors::FSError,
    info::FileLikeInfo,
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
        usize::try_from(meta.len()).map_err(|_| FSError::Other)
    }
}

impl File for Ext4File {
    fn read(&mut self, buffer: &mut [u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.read_bytes(buffer).map_err(|_| FSError::Other)
    }

    fn write(&mut self, buffer: &[u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.write_bytes(buffer).map_err(|_| FSError::Other)
    }

    fn info(&mut self) -> crate::filesystem::vfs::FSResult<FileLikeInfo> {
        let size = self.size()?;
        Ok(FileLikeInfo::new(
            self.name.clone(),
            size,
            FileLikeType::File,
        ))
    }
}

