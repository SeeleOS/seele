use crate::{misc::error::AsSyscallError, systemcall::utils::SyscallError};

#[derive(Debug)]
pub enum SocketError {
    TryAgain,
    InvalidArguments,
    AddressFamilyNotSupported,
    ProtocolNotSupported,
    AddressInUse,
    IsConnected,
    ConnectionRefused,
    BrokenPipe,
}

pub type SocketResult<T> = Result<T, SocketError>;

impl AsSyscallError for SocketError {
    fn as_syscall_error(&self) -> SyscallError {
        match self {
            Self::TryAgain => SyscallError::TryAgain,
            Self::InvalidArguments => SyscallError::InvalidArguments,
            Self::AddressFamilyNotSupported => SyscallError::AddressFamilyNotSupported,
            Self::ProtocolNotSupported => SyscallError::ProtocolNotSupported,
            Self::AddressInUse => SyscallError::AddressInUse,
            Self::IsConnected => SyscallError::IsConnected,
            Self::ConnectionRefused => SyscallError::ConnectionRefused,
            Self::BrokenPipe => SyscallError::BrokenPipe,
        }
    }
}
