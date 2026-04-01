use crate::{
    filesystem::errors::FSError, misc::error::AsSyscallError, systemcall::utils::SyscallError,
};
use crate::socket::SocketError;

#[derive(Debug)]
pub enum ObjectError {
    DoesNotExist,
    TryAgain,
    InvalidRequest,
    InvalidArguments,
    SocketError(SocketError),
    FSError(FSError),
    Other,
}

impl From<FSError> for ObjectError {
    fn from(value: FSError) -> Self {
        Self::FSError(value)
    }
}

impl From<SocketError> for ObjectError {
    fn from(value: SocketError) -> Self {
        Self::SocketError(value)
    }
}

impl AsSyscallError for ObjectError {
    fn as_syscall_error(&self) -> SyscallError {
        match self {
            Self::InvalidArguments => SyscallError::InvalidArguments,
            Self::TryAgain => SyscallError::TryAgain,
            Self::DoesNotExist => SyscallError::BadFileDescriptor,
            Self::InvalidRequest => SyscallError::InappropriateIoctl,
            Self::SocketError(err) => err.as_syscall_error(),
            Self::FSError(err) => err.as_syscall_error(),
            Self::Other => SyscallError::other("object error other"),
        }
    }
}
