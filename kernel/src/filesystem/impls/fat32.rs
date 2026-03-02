use fatfs::{IoBase, Read, ReadWriteSeek, Seek, Write};

use crate::filesystem::{
    block_device::BlockDeviceError,
    storage_operator::{SeekFrom, StorageOperator, initrd::RamDiskReader},
    vfs::FileSystem,
};

pub struct FAT32 {
    fs: fatfs::FileSystem<Fat32RamDiskReader>,
}

pub struct Fat32RamDiskReader {
    inner: RamDiskReader,
}

impl IoBase for Fat32RamDiskReader {
    type Error = BlockDeviceError;
}

impl Write for Fat32RamDiskReader {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Read for Fat32RamDiskReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.inner.read(buf)
    }
}

impl Seek for Fat32RamDiskReader {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        self.inner.seek(SeekFrom::from(pos))
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

impl FileSystem for FAT32 {
    fn init(&mut self) -> crate::filesystem::vfs::FSResult<()> {
        Ok(())
    }
}
