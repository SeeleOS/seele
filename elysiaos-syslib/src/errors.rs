#[derive(Debug)]
pub enum SyscallError {
    InvalidSyscall = -38,
    InvalidFileDescriptor = -255,
    Other = -256,
}

impl From<isize> for SyscallError {
    fn from(value: isize) -> Self {
        match value {
            -38 => Self::InvalidSyscall,
            -255 => Self::InvalidFileDescriptor,
            _ => SyscallError::Other,
        }
    }
}
