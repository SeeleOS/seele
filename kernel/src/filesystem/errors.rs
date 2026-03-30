use crate::{
    filesystem::block_device::BlockDeviceError, misc::error::AsSyscallError,
};
use seele_sys::errors::SyscallError;

#[derive(Clone, Copy, Debug)]
pub enum FSError {
    NotFound,
    NotADirectory,
    NotAFile,
    Readonly,
    Other,
    StorageDeviceError(BlockDeviceError),
}

impl AsSyscallError for FSError {
    fn as_syscall_error(&self) -> SyscallError {
        match self {
            Self::NotFound => SyscallError::FileNotFound,
            Self::NotADirectory => SyscallError::NotADirectory,
            Self::NotAFile => SyscallError::IsADirectory,
            Self::Readonly => SyscallError::ReadOnlyFileSystem,

            Self::StorageDeviceError(err) => err.as_syscall_error(),

            Self::Other => SyscallError::other("FS error other"),
        }
    }
}
