use alloc::vec::Vec;

use crate::{
    define_syscall,
    memory::user_safe,
    object::{misc::ObjectRef, traits::Writable},
    systemcall::utils::{SyscallError, SyscallImpl},
};

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxIovec {
    iov_base: *const u8,
    iov_len: usize,
}

define_syscall!(Vmsplice, |fd: ObjectRef,
                           iov_ptr: *const LinuxIovec,
                           nr_segs: i32,
                           flags: u32| {
    const SPLICE_F_NONBLOCK: u32 = 1;
    if flags & !SPLICE_F_NONBLOCK != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if nr_segs < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let pipe = fd.as_pipe()?;
    if nr_segs == 0 {
        return Ok(0);
    }
    if iov_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut total = 0usize;
    let mut buffer = Vec::new();
    for index in 0..nr_segs as usize {
        let iov = user_safe::read(unsafe { iov_ptr.add(index) })?;
        total = total
            .checked_add(iov.iov_len)
            .ok_or(SyscallError::InvalidArguments)?;
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }
        buffer.extend_from_slice(&user_safe::read_buffer(iov.iov_base, iov.iov_len)?);
    }

    if total == 0 || buffer.is_empty() {
        return Ok(0);
    }

    pipe.write(&buffer).map_err(SyscallError::from)
});
