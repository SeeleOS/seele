use crate::{
    filesystem::{
        block_device::{BlockDevice, BlockDeviceError, initrd::RAMDISK},
        storage_operator::{SeekFrom, StorageOperator},
        vfs_traits::FileSystem,
    },
    s_println,
};

#[derive(Debug)]
pub struct RamDiskReader {
    pos: u64,
    cache: [u8; 1024],
}

impl StorageOperator for RamDiskReader {
    type Error = BlockDeviceError;
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let ramdisk = RAMDISK.get().unwrap();
        let n = ramdisk.read_by_bytes(self.pos as usize, buf)?;

        self.pos += n as u64;

        Ok(n)
    }
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        Err(BlockDeviceError::Readonly)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let ramdisk = RAMDISK.get().unwrap();

        let new_pos: i64 = match pos {
            SeekFrom::Start(s) => s as i64,
            SeekFrom::Current(c) => self.pos as i64 + c,
            SeekFrom::End(e) => ramdisk.total_bytes() as i64 + e,
        };

        if new_pos < 0 || new_pos > ramdisk.total_bytes() as i64 {
            return Err(BlockDeviceError::Other);
        }

        self.pos = new_pos as u64;
        Ok(self.pos)
    }
}
