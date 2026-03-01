pub enum BlockDeviceError {
    Other,
}

pub type BlockDeviceResult = Result<(), BlockDeviceError>;

pub trait BlockDevice: Send + Sync {
    fn block_size(&self) -> u64;
    fn read(&self, id: u64, buffer: &mut [u8]) -> BlockDeviceResult;
    fn write(&self, id: u64, buffer: &[u8]) -> BlockDeviceResult;
}
