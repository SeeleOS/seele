use alloc::string::String;

use crate::{
    filesystem::info::LinuxStat, misc::others::from_cstr, systemcall::error::SyscallError,
};

pub trait SyscallArg {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized;
}

impl SyscallArg for i32 {
    fn from_u64(val: u64) -> Result<Self, SyscallError> {
        Ok(val as i32)
    }
}

impl SyscallArg for u32 {
    fn from_u64(val: u64) -> Result<Self, SyscallError> {
        Ok(val as u32)
    }
}

impl SyscallArg for usize {
    fn from_u64(val: u64) -> Result<Self, SyscallError> {
        Ok(val as usize)
    }
}

impl SyscallArg for String {
    fn from_u64(val: u64) -> Result<Self, SyscallError> {
        unsafe { Ok(from_cstr(val as *const u8)?) }
    }
}

// 处理指针（如 LinuxStat）
impl SyscallArg for *mut LinuxStat {
    fn from_u64(val: u64) -> Result<Self, SyscallError> {
        Ok(val as *mut LinuxStat)
    }
}

impl SyscallArg for u64 {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized,
    {
        Ok(val)
    }
}

impl SyscallArg for *mut u8 {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized,
    {
        Ok(val as *mut u8)
    }
}

impl SyscallArg for bool {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized,
    {
        Ok(val != 0)
    }
}
