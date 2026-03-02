use alloc::{string::String, sync::Arc, vec::Vec};
use fatfs::{IoBase, Read, Seek, Write};
use spin::mutex::Mutex;

use crate::filesystem::{
    block_device::BlockDeviceError,
    errors::FSError,
    storage_operator::{SeekFrom, StorageOperator, initrd::RamDiskOperator},
    vfs_traits::{
        Directory, DirectoryContentInfo, DirectoryContentType, File, FileLike, FileSystem,
    },
};

pub struct Fat32RamDiskReader(pub RamDiskOperator);

impl IoBase for Fat32RamDiskReader {
    type Error = BlockDeviceError;
}

impl Write for Fat32RamDiskReader {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Read for Fat32RamDiskReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf)
    }
}

impl Seek for Fat32RamDiskReader {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        self.0.seek(SeekFrom::from(pos))
    }
}

impl From<fatfs::SeekFrom> for SeekFrom {
    fn from(value: fatfs::SeekFrom) -> Self {
        match value {
            fatfs::SeekFrom::Start(val) => Self::Start(val),
            fatfs::SeekFrom::End(val) => Self::End(val),
            fatfs::SeekFrom::Current(val) => Self::Current(val),
        }
    }
}
