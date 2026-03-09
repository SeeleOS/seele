use crate::{
    filesystem::{block_device::BlockDeviceError, errors::FSError},
    object::error::ObjectError,
    systemcall::error::SyscallError,
};

pub type KernelResult<T> = Result<T, KernelError>;

pub enum KernelError {
    FileSystem(FSError),
    BlockDevice(BlockDeviceError),
    Object(ObjectError),
}

impl From<FSError> for KernelError {
    fn from(value: FSError) -> Self {
        Self::FileSystem(value)
    }
}

impl From<BlockDeviceError> for KernelError {
    fn from(value: BlockDeviceError) -> Self {
        Self::BlockDevice(value)
    }
}

impl From<ObjectError> for KernelError {
    fn from(value: ObjectError) -> Self {
        Self::Object(value)
    }
}

pub trait AsSyscallError {
    fn as_syscall_error(&self) -> SyscallError;
}

impl KernelError {
    pub fn as_syscall_error(self) -> SyscallError {
        match self {
            Self::FileSystem(err) => err.as_syscall_error(),
            Self::BlockDevice(err) => err.as_syscall_error(),
            Self::Object(err) => err.as_syscall_error(),
        }
    }
}
