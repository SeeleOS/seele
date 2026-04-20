use crate::{
    filesystem::block_device::BlockDeviceError, misc::error::AsSyscallError,
    systemcall::utils::SyscallError,
};

#[derive(Clone, Copy, Debug)]
pub enum FSError {
    NotFound,
    NotADirectory,
    NotAFile,
    NotASymlink,
    AlreadyExists,
    Busy,
    DirectoryNotEmpty,
    NoSpace,
    AccessDenied,
    PathTooLong,
    TooManySymlinks,
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
            Self::NotASymlink => SyscallError::InvalidArguments,
            Self::AlreadyExists => SyscallError::FileAlreadyExists,
            Self::Busy => SyscallError::DeviceOrResourceBusy,
            Self::DirectoryNotEmpty => SyscallError::DirectoryNotEmpty,
            Self::NoSpace => SyscallError::NoSpaceLeft,
            Self::AccessDenied => SyscallError::AccessDenied,
            Self::PathTooLong => SyscallError::PathTooLong,
            Self::TooManySymlinks => SyscallError::TooManySymbolicLinks,

            Self::Readonly => SyscallError::ReadOnlyFileSystem,

            Self::StorageDeviceError(err) => err.as_syscall_error(),

            Self::Other => SyscallError::IOError,
        }
    }
}
