use crate::{misc::error::AsSyscallError, systemcall::utils::SyscallError};

#[derive(Debug)]
pub enum SocketError {
    TryAgain,
    InvalidArguments,
    OperationNotSupported,
    AddressFamilyNotSupported,
    ProtocolNotSupported,
    AddressInUse,
    AddressNotAvailable,
    NetworkDown,
    IsConnected,
    NotConnected,
    ConnectionRefused,
    BrokenPipe,
}

pub type SocketResult<T> = Result<T, SocketError>;

impl AsSyscallError for SocketError {
    fn as_syscall_error(&self) -> SyscallError {
        match self {
            Self::TryAgain => SyscallError::TryAgain,
            Self::InvalidArguments => SyscallError::InvalidArguments,
            Self::OperationNotSupported => SyscallError::OperationNotSupported,
            Self::AddressFamilyNotSupported => SyscallError::AddressFamilyNotSupported,
            Self::ProtocolNotSupported => SyscallError::ProtocolNotSupported,
            Self::AddressInUse => SyscallError::AddressInUse,
            Self::AddressNotAvailable => SyscallError::AddressNotAvailable,
            Self::NetworkDown => SyscallError::NetworkDown,
            Self::IsConnected => SyscallError::IsConnected,
            Self::NotConnected => SyscallError::NotConnected,
            Self::ConnectionRefused => SyscallError::ConnectionRefused,
            Self::BrokenPipe => SyscallError::BrokenPipe,
        }
    }
}
