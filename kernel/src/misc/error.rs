use crate::{
    filesystem::{block_device::BlockDeviceError, errors::FSError},
    object::error::ObjectError,
    systemcall::utils::SyscallError,
};

pub type KernelResult<T> = Result<T, KernelError>;

pub enum KernelError {
    FileSystem(FSError),
    BlockDevice(BlockDeviceError),
    Object(ObjectError),

    InvalidString,
}

macro_rules! register_error {
    ($err_type:ty, $kerror_type: ident) => {
        impl From<$err_type> for SyscallError {
            fn from(err: $err_type) -> Self {
                KernelError::from(err).as_syscall_error()
            }
        }

        impl From<$err_type> for KernelError {
            fn from(err: $err_type) -> Self {
                Self::$kerror_type(err)
            }
        }
    };
}

register_error!(FSError, FileSystem);
register_error!(BlockDeviceError, BlockDevice);
register_error!(ObjectError, Object);

pub trait AsSyscallError {
    fn as_syscall_error(&self) -> SyscallError;
}

impl KernelError {
    pub fn as_syscall_error(self) -> SyscallError {
        match self {
            Self::FileSystem(err) => err.as_syscall_error(),
            Self::BlockDevice(err) => err.as_syscall_error(),
            Self::Object(err) => err.as_syscall_error(),
            Self::InvalidString => SyscallError::InvalidArguments,
        }
    }
}
