use alloc::{string::String, sync::Arc, vec::Vec};
use fatfs::{IoBase, Read, Seek, Write};
use spin::mutex::Mutex;

use crate::filesystem::{
    block_device::BlockDeviceError,
    errors::FSError,
    impls::fat32::operator::Fat32RamDiskReader,
    storage_operator::{SeekFrom, StorageOperator, initrd::RamDiskOperator},
    vfs_traits::{
        Directory, DirectoryContentInfo, DirectoryContentType, File, FileLike, FileSystem,
    },
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
}

impl FAT32File {
    pub fn new(name: String, inner: RawFAT32File) -> Self {
        Self { name, inner }
    }
}

impl File for FAT32File {
    fn read(&mut self, buffer: &mut [u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.read(buffer).map_err(|_| FSError::NotFound)
    }

    fn write(&mut self, buffer: &[u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.write(buffer).map_err(|_| FSError::NotFound)
    }

    fn name(&mut self) -> crate::filesystem::vfs::FSResult<String> {
        Ok(self.name.clone())
    }
}
