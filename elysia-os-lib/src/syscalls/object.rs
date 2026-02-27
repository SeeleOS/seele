use crate::{syscall, utils::SyscallResult};

pub fn read_object(index: u64, buffer: &mut [u8]) -> SyscallResult {
    syscall!(
        ReadObject,
        index,
        buffer.as_ptr() as u64,
        buffer.len() as u64
    )
}

pub fn write_object(index: u64, buffer: &[u8]) -> SyscallResult {
    syscall!(
        WriteObject,
        index,
        buffer.as_ptr() as u64,
        buffer.len() as u64
    )
}
