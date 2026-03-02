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

#[derive(Debug)]
pub struct RamDiskReader {
    pos: u64,
    cache: [u8; 1024],
}

impl IoBase for RamDiskReader {
    type Error = BlockDeviceError;
}

impl Read for RamDiskReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let ramdisk = RAMDISK.get().unwrap();
        let b_size = ramdisk.block_size() as u64;

        let block_id = (self.pos / b_size) as usize;
        let offset_in_block = (self.pos % b_size) as usize;

        ramdisk.read(block_id, &mut self.cache)?;

        let available_in_block = (b_size as usize) - offset_in_block;

        let n = core::cmp::min(buf.len(), available_in_block);

        buf[..n].copy_from_slice(&self.cache[offset_in_block..offset_in_block + n]);

        self.pos += n as u64;

        Ok(n)
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
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        let ramdisk = RAMDISK.get().unwrap();
        // 总字节数 = 总块数 * 每块大小
        let total_size = (ramdisk.total_blocks() * ramdisk.block_size()) as i64;

        let new_pos: i64 = match pos {
            fatfs::SeekFrom::Start(s) => s as i64,
            fatfs::SeekFrom::Current(c) => self.pos as i64 + c,
            fatfs::SeekFrom::End(e) => total_size + e,
        };

        if new_pos < 0 || new_pos > total_size {
            return Err(BlockDeviceError::InvalidOffset);
        }

        self.pos = new_pos as u64;
        Ok(self.pos)
    }
}

impl ReadWriteSeek for RamDiskReader {}

impl FileSystem for FAT32 {
    fn init(&mut self) -> crate::filesystem::vfs::FSResult<()> {
        Ok(())
    }
}
