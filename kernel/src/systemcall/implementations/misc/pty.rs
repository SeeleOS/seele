use super::*;

define_syscall!(CreatePty, |master_ptr: *mut i32, slave_ptr: *mut i32| {
    if master_ptr.is_null() || slave_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let (master, slave) = create_pty();
    user_safe::write(master_ptr, &master)?;
    user_safe::write(slave_ptr, &slave)?;
    Ok(0)
});
