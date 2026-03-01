use crate::{syscall, utils::SyscallResult, wrap_c};

wrap_c!(read_object(index: u64, buffer: &mut [u8]));
pub fn read_object(index: u64, buffer: &mut [u8]) -> SyscallResult {
    syscall!(
        ReadObject,
        index,
        buffer.as_ptr() as u64,
        buffer.len() as u64
    )
}

wrap_c!(write_object(index: u64, buffer: &[u8]));
pub fn write_object(index: u64, buffer: &[u8]) -> SyscallResult {
    syscall!(
        WriteObject,
        index,
        buffer.as_ptr() as u64,
        buffer.len() as u64
    )
}

wrap_c!(configurate_object(index: u64, request_num: u64, ptr: *mut u8));
pub fn configurate_object(index: u64, request_num: u64, ptr: *mut u8) -> SyscallResult {
    syscall!(ConfigurateObject, index, request_num, ptr as u64)
}
