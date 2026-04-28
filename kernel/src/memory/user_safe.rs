use alloc::vec::Vec;

use crate::{
    process::manager::get_current_process,
    systemcall::utils::{SyscallError, SyscallResult},
};

pub fn write<T: ?Sized, U>(ptr: *mut U, value: &T) -> SyscallResult<()> {
    if ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    get_current_process().lock().addrspace.write(ptr, value)
}

pub fn read<T: Copy>(ptr: *const T) -> SyscallResult<T> {
    if ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    get_current_process().lock().addrspace.read(ptr)
}

pub fn read_buffer(ptr: *const u8, len: usize) -> SyscallResult<Vec<u8>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    get_current_process().lock().addrspace.read_buffer(ptr, len)
}

pub fn write_buffer(ptr: *mut u8, bytes: &[u8]) -> SyscallResult<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    if ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    get_current_process()
        .lock()
        .addrspace
        .write_buffer(ptr, bytes)
}
