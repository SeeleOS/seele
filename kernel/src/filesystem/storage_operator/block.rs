use alloc::sync::Arc;

use crate::filesystem::{
    block_device::{BlockDevice, BlockDeviceError},
    storage_operator::{SeekFrom, StorageOperator},
};

pub struct BlockDeviceOperator {
    device: Arc<dyn BlockDevice>,
    pos: u64,
}

impl BlockDeviceOperator {
    pub fn new(device: Arc<dyn BlockDevice>) -> Self {
        Self { device, pos: 0 }
    }
}

impl StorageOperator for BlockDeviceOperator {
    type Error = BlockDeviceError;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let n = self.device.read_by_bytes(self.pos as usize, buf)?;
        self.pos += n as u64;
        Ok(n)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let n = self.device.write_by_bytes(self.pos as usize, buf)?;
        self.pos += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.device.flush()
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let new_pos: i64 = match pos {
            SeekFrom::Start(s) => s as i64,
            SeekFrom::Current(c) => self.pos as i64 + c,
            SeekFrom::End(e) => self.device.total_bytes() as i64 + e,
        };

        if new_pos < 0 || new_pos > self.device.total_bytes() as i64 {
            return Err(BlockDeviceError::Other);
        }

        self.pos = new_pos as u64;
        Ok(self.pos)
    }
}
