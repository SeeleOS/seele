use fatfs::IoError;

#[derive(Debug)]
pub enum BlockDeviceError {
    Readonly,
    OutOfBounds,
    BufferTooSmall,
    Other,
}

impl IoError for BlockDeviceError {
    fn is_interrupted(&self) -> bool {
        true
    }

    fn new_unexpected_eof_error() -> Self {
        Self::OutOfBounds
    }

    fn new_write_zero_error() -> Self {
        Self::Other
    }
}

pub type BlockDeviceResult = Result<usize, BlockDeviceError>;

pub trait BlockDevice: Send + Sync {
    fn block_size(&self) -> usize;
    fn read_block(&self, id: usize, buffer: &mut [u8]) -> BlockDeviceResult;
    fn write_block(&self, id: usize, buffer: &[u8]) -> BlockDeviceResult;
}
