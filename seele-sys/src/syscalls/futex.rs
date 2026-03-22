use crate::{errors::SyscallError, syscall, utils::SyscallResult};

pub enum WaitResultType {
    Success = 1,
    TryAgain,
    Invalid,
}

impl From<usize> for WaitResultType {
    fn from(value: usize) -> Self {
        match value {
            1 => Self::Success,
            2 => Self::TryAgain,
            _ => Self::Invalid,
        }
    }
}

pub fn wait(addr: *mut u32, expected_value: u64) -> SyscallResult {
    syscall!(FutexWait, addr as u64, expected_value)
}

pub fn wake(addr: *mut u32, count: u64) -> SyscallResult {
    syscall!(FutexWake, addr as u64, count)
}
