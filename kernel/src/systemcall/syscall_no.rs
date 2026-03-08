use num_enum::TryFromPrimitive;

#[derive(Debug, Clone, Copy, PartialEq, TryFromPrimitive)]
#[repr(usize)]
pub enum SyscallNo {
    Print = 1000,
    SetFs,
    SetGs,
    GetFs,
    AllocateMem,
    GetProcessID,
    GetThreadID,
    FutexWait,
    FutexWake,
    Exit,
    ReadObject,
    WriteObject,
    ConfigurateObject,
    ChangeDirectory,
    GetCurrentDirectory,
    FileInfo,
    Fork,
    Execve,
    OpenFile,
}
