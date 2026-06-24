use thiserror::Error;

use crate::socket::SocketError;
use crate::{
    filesystem::errors::FSError, misc::error::AsSyscallError, systemcall::utils::SyscallError,
};

#[derive(Debug, Error)]
pub enum ObjectError {
    #[error("object does not exist")]
    DoesNotExist,
    #[error("bad address")]
    BadAddress,
    #[error("operation interrupted")]
    Interrupted,
    #[error("operation would block")]
    TryAgain,
    #[error("resource busy")]
    Busy,
    #[error("device revoked")]
    DeviceRevoked,
    #[error("invalid object request")]
    InvalidRequest,
    #[error("too many open files")]
    TooManyOpenFilesProcess,
    #[error("invalid object arguments")]
    InvalidArguments,
    #[error("operation not implemented")]
    Unimplemented,
    #[error(transparent)]
    SocketError(#[from] SocketError),
    #[error(transparent)]
    FSError(#[from] FSError),
    #[error("object I/O failed")]
    Other,
}

impl AsSyscallError for ObjectError {
    fn as_syscall_error(&self) -> SyscallError {
        match self {
            Self::Unimplemented => SyscallError::OperationNotSupported,
            Self::BadAddress => SyscallError::BadAddress,
            Self::InvalidArguments => SyscallError::InvalidArguments,
            Self::Interrupted => SyscallError::Interrupted,
            Self::TryAgain => SyscallError::TryAgain,
            Self::Busy => SyscallError::DeviceOrResourceBusy,
            Self::DeviceRevoked => SyscallError::NoDevice,
            Self::DoesNotExist => SyscallError::BadFileDescriptor,
            Self::InvalidRequest => SyscallError::InappropriateIoctl,
            Self::TooManyOpenFilesProcess => SyscallError::TooManyOpenFilesProcess,
            Self::SocketError(err) => err.as_syscall_error(),
            Self::FSError(err) => err.as_syscall_error(),
            Self::Other => SyscallError::IOError,
        }
    }
}
