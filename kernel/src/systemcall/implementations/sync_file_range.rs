use crate::{
    define_syscall,
    object::{Object, misc::ObjectRef},
    systemcall::utils::{SyscallError, SyscallImpl},
};

define_syscall!(SyncFileRange, |fd: ObjectRef,
                                offset: i64,
                                nbytes: i64,
                                flags: u32| {
    const VALID_FLAGS: u32 = 1 | 2 | 4;
    if flags & !VALID_FLAGS != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if offset < 0 || nbytes < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let file_like = fd.as_file_like();
    match file_like {
        Ok(file_like) => {
            if file_like.as_seekable().is_err() {
                return Err(SyscallError::IllegalSeek);
            }
        }
        Err(err) => return Err(err),
    }
    Ok(0)
});
