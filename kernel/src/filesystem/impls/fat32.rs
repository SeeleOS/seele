use fatfs::{IoBase, Read, ReadWriteSeek, Seek, Write};

use crate::{
    filesystem::{
        block_device::{BlockDevice, BlockDeviceError},
        vfs::FileSystem,
    },
    keyboard::block_device::initrd::RAMDISK,
};

pub struct FAT32 {
    fs: fatfs::FileSystem<RamDiskReader>,
}

#[derive(Default, Debug)]
pub struct RamDiskReader {
    pos: u64,
}

impl IoBase for RamDiskReader {
    type Error = BlockDeviceError;
}

impl Read for RamDiskReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let ramdisk = RAMDISK.get().unwrap();
        let pos = self.pos * ramdisk.block_size() as u64;
        let result = ramdisk.read(pos as usize, buf)?;

        self.pos += result as u64;

        Ok(result)
    }
}

impl Write for RamDiskReader {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        Err(BlockDeviceError::Readonly)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Seek for RamDiskReader {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {}
}

impl ReadWriteSeek for RamDiskReader {}

impl FileSystem for FAT32 {
    fn init(&mut self) -> crate::filesystem::vfs::FSResult<()> {}
}
