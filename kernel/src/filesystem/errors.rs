use crate::{
    filesystem::block_device::BlockDeviceError, misc::error::AsSyscallError,
    systemcall::error::SyscallError,
};

#[derive(Clone, Copy, Debug)]
pub enum FSError {
    NotFound,
    NotADirectory,
    NotAFile,
    Other,
    StorageDeviceError(BlockDeviceError),
}

impl AsSyscallError for FSError {
    fn as_syscall_error(&self) -> crate::systemcall::error::SyscallError {
        match self {
            _ => SyscallError::Other,
        }
    }
}
