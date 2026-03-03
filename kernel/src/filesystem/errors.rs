use crate::filesystem::block_device::BlockDeviceError;

#[derive(Clone, Copy, Debug)]
pub enum FSError {
    NotFound,
    NotADirectory,
    NotAFile,
    StorageDeviceError(BlockDeviceError),
}
