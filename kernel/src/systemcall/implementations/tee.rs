use crate::define_syscall;
use crate::{
    object::misc::ObjectRef,
    systemcall::utils::{SyscallError, SyscallImpl},
};

define_syscall!(Tee, |fd_in: ObjectRef,
                      fd_out: ObjectRef,
                      len: usize,
                      flags: u32| {
    const SPLICE_F_NONBLOCK: u32 = 1;
    if flags & !SPLICE_F_NONBLOCK != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let input = fd_in.as_pipe()?;
    let output = fd_out.as_pipe()?;
    if len == 0 {
        return Ok(0);
    }
    input.tee_to(&output, len)
});
