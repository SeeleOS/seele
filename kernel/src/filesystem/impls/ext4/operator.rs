use alloc::boxed::Box;
use core::error::Error;

use ext4plus::{Ext4Read, Ext4Write};
use spin::mutex::Mutex;

use crate::filesystem::{
    block_device::BlockDeviceError,
    storage_operator::{SeekFrom, StorageOperator, initrd::RamDiskOperator},
};

/// Simple adapter that lets ext4plus read from the existing ramdisk
/// storage operator.
pub struct Ext4RamDiskReader(pub Mutex<RamDiskOperator>);

#[derive(Debug)]
struct Ext4RamDiskIoError(BlockDeviceError);

impl core::fmt::Display for Ext4RamDiskIoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ext4 ramdisk IO error: {:?}", self.0)
    }
}

impl Error for Ext4RamDiskIoError {}

impl From<BlockDeviceError> for Ext4RamDiskIoError {
    fn from(err: BlockDeviceError) -> Self {
        Self(err)
    }
}

impl Ext4Read for Ext4RamDiskReader {
    fn read(
        &self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let mut op = self.0.lock();

        op.seek(SeekFrom::Start(start_byte))
            .map_err(Ext4RamDiskIoError::from)?;

        let n = op.read(dst).map_err(Ext4RamDiskIoError::from)?;

        if n != dst.len() {
            // ext4plus expects the buffer to be fully filled.
            return Err(Box::new(Ext4RamDiskIoError(BlockDeviceError::Other)));
        }

        Ok(())
    }
}

impl Ext4Write for Ext4RamDiskReader {
    fn write(
        &self,
        start_byte: u64,
        src: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let mut op = self.0.lock();

        op.seek(SeekFrom::Start(start_byte))
            .map_err(Ext4RamDiskIoError::from)?;

        let n = op.write(src).map_err(Ext4RamDiskIoError::from)?;

        if n != src.len() {
            return Err(Box::new(Ext4RamDiskIoError(BlockDeviceError::Other)));
        }

        op.flush().map_err(Ext4RamDiskIoError::from)?;
        Ok(())
    }
}
