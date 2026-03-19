use crate::{syscall, utils::SyscallResult};

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    SetFlags = 0,
    GetFlags = 1,
}

impl From<i32> for Command {
    fn from(value: i32) -> Self {
        Self::from(value as u64)
    }
}

impl From<u64> for Command {
    fn from(value: u64) -> Self {
        match value {
            0 => Self::SetFlags,
            1 => Self::GetFlags,
            _ => panic!("invalid object control command: {value}"),
        }
    }
}

pub fn read_object(object: u64, buffer: &mut [u8]) -> SyscallResult {
    syscall!(
        ReadObject,
        object,
        buffer.as_ptr() as u64,
        buffer.len() as u64
    )
}

pub fn write_object(object: u64, buffer: &[u8]) -> SyscallResult {
    syscall!(
        WriteObject,
        object,
        buffer.as_ptr() as u64,
        buffer.len() as u64
    )
}

pub fn configurate_object(object: u64, request_num: u64, ptr: *mut u8) -> SyscallResult {
    syscall!(ConfigurateObject, object, request_num, ptr as u64)
}

pub fn control_object_raw(object: u64, command: u64, arg: u64) -> SyscallResult {
    syscall!(ControlObject, object, command as u64, arg)
}

pub fn control_object(object: u64, command: Command, arg: u64) -> SyscallResult {
    control_object_raw(object, command as u64, arg)
}

pub fn remove_object(object: u64) -> SyscallResult {
    syscall!(RemoveObject, object)
}
