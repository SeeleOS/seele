use crate::filesystem::{block_device::BlockDeviceError, errors::FSError};

pub type KernelResult<T> = Result<T, KernelError>;

pub enum KernelError {
    FileSystem(FSError),
    BlockDevice(BlockDeviceError),
}

impl From<FSError> for KernelError {
    fn from(value: FSError) -> Self {
        Self::FileSystem(value)
    }
}

impl From<BlockDeviceError> for KernelError {
    fn from(value: BlockDeviceError) -> Self {
        Self::BlockDevice(value)
    }
}
