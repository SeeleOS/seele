use alloc::string::String;
use fatfs::{Read, Write};

use crate::filesystem::{
    errors::FSError,
    impls::fat32::operator::Fat32RamDiskReader,
    info::FileLikeInfo,
    vfs_traits::{File, FileLikeType},
};

type RawFAT32File = fatfs::File<
    'static,
    Fat32RamDiskReader,
    fatfs::DefaultTimeProvider,
    fatfs::LossyOemCpConverter,
>;

pub struct FAT32File {
    name: String,
    inner: RawFAT32File,
    size: usize,
}

impl FAT32File {
    pub fn new(name: String, inner: RawFAT32File, size: usize) -> Self {
        Self { name, inner, size }
    }
}

impl File for FAT32File {
    fn read(&mut self, buffer: &mut [u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.read(buffer).map_err(|_| FSError::Other)
    }

    fn write(&mut self, buffer: &[u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.write(buffer).map_err(|_| FSError::Other)
    }

    fn info(&mut self) -> crate::filesystem::vfs::FSResult<FileLikeInfo> {
        log::trace!("fat32 file info");
        Ok(FileLikeInfo::new(
            self.name.clone(),
            self.size,
            FileLikeType::File,
        ))
    }
}
