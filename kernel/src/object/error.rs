use crate::{misc::error::AsSyscallError, systemcall::error::SyscallError};

#[derive(Debug)]
pub enum ObjectError {
    DoesNotExist,
    Other,
}

impl AsSyscallError for ObjectError {
    fn as_syscall_error(&self) -> crate::systemcall::error::SyscallError {
        match self {
            Self::DoesNotExist => SyscallError::BadFileDescriptor,
            Self::Other => SyscallError::other("object error other"),
        }
    }
}
