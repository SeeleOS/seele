use thiserror::Error;

use crate::{misc::error::AsSyscallError, systemcall::utils::SyscallError};

#[derive(Debug, Error)]
pub enum SocketError {
    #[error("operation would block")]
    TryAgain,
    #[error("invalid socket arguments")]
    InvalidArguments,
    #[error("socket operation not supported")]
    OperationNotSupported,
    #[error("address family not supported")]
    AddressFamilyNotSupported,
    #[error("protocol not supported")]
    ProtocolNotSupported,
    #[error("address already in use")]
    AddressInUse,
    #[error("address not available")]
    AddressNotAvailable,
    #[error("network is down")]
    NetworkDown,
    #[error("permission denied")]
    PermissionDenied,
    #[error("socket is already connected")]
    IsConnected,
    #[error("socket is not connected")]
    NotConnected,
    #[error("connection refused")]
    ConnectionRefused,
    #[error("broken pipe")]
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
            Self::PermissionDenied => SyscallError::PermissionDenied,
            Self::IsConnected => SyscallError::IsConnected,
            Self::NotConnected => SyscallError::NotConnected,
            Self::ConnectionRefused => SyscallError::ConnectionRefused,
            Self::BrokenPipe => SyscallError::BrokenPipe,
        }
    }
}
