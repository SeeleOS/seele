use crate::{
    filesystem::errors::FSError, misc::error::AsSyscallError, systemcall::utils::SyscallError,
};

#[derive(Debug)]
pub enum ObjectError {
    DoesNotExist,
    TryAgain,
    InvalidRequest,
    InvalidArguments,
    AddressFamilyNotSupported,
    ProtocolNotSupported,
    OperationNotSupported,
    AddressInUse,
    NotConnected,
    IsConnected,
    ConnectionRefused,
    BrokenPipe,
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
            Self::InvalidArguments => SyscallError::InvalidArguments,
            Self::TryAgain => SyscallError::TryAgain,
            Self::DoesNotExist => SyscallError::BadFileDescriptor,
            Self::InvalidRequest => SyscallError::InappropriateIoctl,
            Self::AddressFamilyNotSupported => SyscallError::AddressFamilyNotSupported,
            Self::ProtocolNotSupported => SyscallError::ProtocolNotSupported,
            Self::OperationNotSupported => SyscallError::OperationNotSupported,
            Self::AddressInUse => SyscallError::AddressInUse,
            Self::NotConnected => SyscallError::NotConnected,
            Self::IsConnected => SyscallError::IsConnected,
            Self::ConnectionRefused => SyscallError::ConnectionRefused,
            Self::BrokenPipe => SyscallError::BrokenPipe,
            Self::FSError(err) => err.as_syscall_error(),
            Self::Other => SyscallError::other("object error other"),
        }
    }
}
