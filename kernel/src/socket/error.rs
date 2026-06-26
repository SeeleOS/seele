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
    #[error("socket type not supported")]
    SocketTypeNotSupported,
    #[error("address family not supported")]
    AddressFamilyNotSupported,
    #[error("protocol not supported")]
    ProtocolNotSupported,
    #[error("protocol option not supported")]
    ProtocolOptionNotSupported,
    #[error("address already in use")]
    AddressInUse,
    #[error("address not available")]
    AddressNotAvailable,
    #[error("network is down")]
    NetworkDown,
    #[error("not a directory")]
    NotADirectory,
    #[error("I/O error")]
    IoError,
    #[error("access denied")]
    AccessDenied,
    #[error("permission denied")]
    PermissionDenied,
    #[error("socket is already connected")]
    IsConnected,
    #[error("socket is not connected")]
    NotConnected,
    #[error("message is too long")]
    MessageTooLong,
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
            Self::SocketTypeNotSupported => SyscallError::SocketTypeNotSupported,
            Self::AddressFamilyNotSupported => SyscallError::AddressFamilyNotSupported,
            Self::ProtocolNotSupported => SyscallError::ProtocolNotSupported,
            Self::ProtocolOptionNotSupported => SyscallError::ProtocolOptionNotSupported,
            Self::AddressInUse => SyscallError::AddressInUse,
            Self::AddressNotAvailable => SyscallError::AddressNotAvailable,
            Self::NetworkDown => SyscallError::NetworkDown,
            Self::NotADirectory => SyscallError::NotADirectory,
            Self::IoError => SyscallError::IOError,
            Self::AccessDenied => SyscallError::AccessDenied,
            Self::PermissionDenied => SyscallError::PermissionDenied,
            Self::IsConnected => SyscallError::IsConnected,
            Self::NotConnected => SyscallError::NotConnected,
            Self::MessageTooLong => SyscallError::MessageTooLong,
            Self::ConnectionRefused => SyscallError::ConnectionRefused,
            Self::BrokenPipe => SyscallError::BrokenPipe,
        }
    }
}
