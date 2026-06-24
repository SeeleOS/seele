use thiserror::Error;

use crate::{
    filesystem::block_device::BlockDeviceError, misc::error::AsSyscallError,
    systemcall::utils::SyscallError,
};

#[derive(Clone, Copy, Debug, Error)]
pub enum FSError {
    #[error("file not found")]
    NotFound,
    #[error("not a directory")]
    NotADirectory,
    #[error("not a file")]
    NotAFile,
    #[error("not a symlink")]
    NotASymlink,
    #[error("file already exists")]
    AlreadyExists,
    #[error("resource busy")]
    Busy,
    #[error("directory not empty")]
    DirectoryNotEmpty,
    #[error("no space left on device")]
    NoSpace,
    #[error("illegal seek")]
    IllegalSeek,
    #[error("invalid arguments")]
    InvalidArguments,
    #[error("access denied")]
    AccessDenied,
    #[error("path too long")]
    PathTooLong,
    #[error("exec format error")]
    ExecFormat,
    #[error("too many symlinks")]
    TooManySymlinks,
    #[error("filesystem is read-only")]
    Readonly,
    #[error("filesystem I/O failed")]
    Other,
    #[error(transparent)]
    StorageDeviceError(#[from] BlockDeviceError),
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
            Self::IllegalSeek => SyscallError::IllegalSeek,
            Self::InvalidArguments => SyscallError::InvalidArguments,
            Self::AccessDenied => SyscallError::AccessDenied,
            Self::PathTooLong => SyscallError::PathTooLong,
            Self::ExecFormat => SyscallError::ExecFormatError,
            Self::TooManySymlinks => SyscallError::TooManySymbolicLinks,

            Self::Readonly => SyscallError::ReadOnlyFileSystem,

            Self::StorageDeviceError(err) => err.as_syscall_error(),

            Self::Other => SyscallError::IOError,
        }
    }
}
