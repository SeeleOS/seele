use crate::{misc::error::AsSyscallError, systemcall::error::SyscallError};

#[derive(Debug)]
pub enum ObjectError {
    Other,
}

impl AsSyscallError for ObjectError {
    fn as_syscall_error(&self) -> crate::systemcall::error::SyscallError {
        match self {
            _ => SyscallError::Other,
        }
    }
}
