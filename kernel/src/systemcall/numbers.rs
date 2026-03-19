use num_enum::TryFromPrimitive;
use strum::EnumIter;

#[derive(EnumIter, Debug, Clone, Copy, PartialEq, TryFromPrimitive)]
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
    RemoveObject,
    WaitForProcessExit,
    GetDirectoryContents,
    GetProcessParentID,
}
