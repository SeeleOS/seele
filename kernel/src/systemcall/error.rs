use crate::misc::error::KernelError;

pub enum SyscallError {
    BufferTooSmall = -1,
    InvalidPath = -2,
    InvalidString = -3,
    InvalidSyscall = -38,
    UnconfiguratableObject = -400,
    InvalidFileDescriptor = -255,
    Other = -256,
}

impl From<KernelError> for SyscallError {
    fn from(value: KernelError) -> Self {
        value.as_syscall_error()
    }
}
