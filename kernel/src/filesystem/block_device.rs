pub enum BlockDeviceError {
    Readonly,
    OutOfBounds,
    BufferTooSmall,
    Other,
}

pub type BlockDeviceResult = Result<(), BlockDeviceError>;

pub trait BlockDevice: Send + Sync {
    fn block_size(&self) -> usize;
    fn read(&self, id: usize, buffer: &mut [u8]) -> BlockDeviceResult;
    fn write(&self, id: usize, buffer: &[u8]) -> BlockDeviceResult;
}
