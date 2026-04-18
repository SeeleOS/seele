use crate::{
    process::manager::get_current_process,
    systemcall::utils::{SyscallError, SyscallResult},
};

pub fn write<T>(ptr: *mut T, value: &T) -> SyscallResult<()> {
    if ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    get_current_process().lock().addrspace.write(ptr, value)
}
