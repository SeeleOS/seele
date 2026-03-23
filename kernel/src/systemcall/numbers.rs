use num_enum::TryFromPrimitive;
use strum::EnumIter;

use crate::systemcall::error::SyscallError;

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
    ControlObject,

    CreatePoller,
    PollerAdd,
    PollerRemove,
    PollerWait,

    CloneObject,
    CloneObjectTo,
}

impl SyscallNo {
    pub fn from_number(number: usize) -> Result<Self, SyscallError> {
        Self::try_from(number).map_err(|_| SyscallError::NoSyscall)
    }
}
