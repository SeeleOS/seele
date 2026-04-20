use alloc::{boxed::Box, sync::Arc};
use core::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

use ext4plus::{Ext4Read, Ext4Write};
use spin::mutex::Mutex;

use crate::filesystem::{
    block_device::{BlockDevice, BlockDeviceError},
    storage_operator::{SeekFrom, StorageOperator, block::BlockDeviceOperator},
};

/// Simple adapter that lets ext4plus read from a generic block device.
pub struct Ext4BlockOperator(pub Mutex<BlockDeviceOperator>);

impl Ext4BlockOperator {
    pub fn new(device: Arc<dyn BlockDevice>) -> Self {
        Self(Mutex::new(BlockDeviceOperator::new(device)))
    }
}

/// Backwards-compatible alias for the old initrd-backed ext4 path.
pub type Ext4RamDiskOperator = Ext4BlockOperator;

#[derive(Debug)]
struct Ext4BlockIoError(BlockDeviceError);

impl Display for Ext4BlockIoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "ext4 block IO error: {:?}", self.0)
    }
}

impl Error for Ext4BlockIoError {}

impl From<BlockDeviceError> for Ext4BlockIoError {
    fn from(err: BlockDeviceError) -> Self {
        Self(err)
    }
}

impl Ext4Read for Ext4BlockOperator {
    fn read(
        &self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let mut op = self.0.lock();

        op.seek(SeekFrom::Start(start_byte))
            .map_err(Ext4BlockIoError::from)?;

        let n = op.read(dst).map_err(Ext4BlockIoError::from)?;

        if n != dst.len() {
            // ext4plus expects the buffer to be fully filled.
            return Err(Box::new(Ext4BlockIoError(BlockDeviceError::Other)));
        }

        Ok(())
    }
}

impl Ext4Write for Ext4BlockOperator {
    fn write(
        &self,
        start_byte: u64,
        src: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let mut op = self.0.lock();

        op.seek(SeekFrom::Start(start_byte))
            .map_err(Ext4BlockIoError::from)?;

        let n = op.write(src).map_err(Ext4BlockIoError::from)?;

        if n != src.len() {
            return Err(Box::new(Ext4BlockIoError(BlockDeviceError::Other)));
        }
        Ok(())
    }
}
