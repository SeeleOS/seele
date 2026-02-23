use num_enum::TryFromPrimitive;

#[derive(Debug, Clone, Copy, PartialEq, TryFromPrimitive)]
#[repr(usize)]
pub enum SyscallNo {
    Print = 1,
    SetFs = 2,
    SetGs = 3,
    GetFs = 4,
    AllocateMem = 5,
    GetProcessID = 6,
    GetThreadID = 7,
    FutexWait = 8,
    FutexWake = 9,
    Exit = 10,
}
