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
