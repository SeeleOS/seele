use crate::{
    filesystem::errors::FSError, misc::error::AsSyscallError,
};
use seele_sys::errors::SyscallError;

#[derive(Debug)]
pub enum ObjectError {
    DoesNotExist,
    TryAgain,
    InvalidRequest,
    FSError(FSError),
    Other,
}

impl From<FSError> for ObjectError {
    fn from(value: FSError) -> Self {
        Self::FSError(value)
    }
}

impl AsSyscallError for ObjectError {
    fn as_syscall_error(&self) -> SyscallError {
        match self {
            Self::TryAgain => SyscallError::TryAgain,
            Self::DoesNotExist => SyscallError::BadFileDescriptor,
            Self::InvalidRequest => SyscallError::InappropriateIoctl,
            Self::FSError(err) => err.as_syscall_error(),
            Self::Other => SyscallError::other("object error other"),
        }
    }
}
