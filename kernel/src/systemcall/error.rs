pub enum SyscallError {
    BufferTooSmall = -1,
    InvalidSyscall = -38,
    UnconfiguratableObject = -400,
    InvalidFileDescriptor = -255,
    Other = -256,
}
