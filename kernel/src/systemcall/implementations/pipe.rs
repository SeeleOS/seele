use crate::{
    define_syscall,
    process::manager::get_current_process,
    socket::{AF_UNIX, SOCK_NONBLOCK, SOCK_STREAM, UnixSocketObject},
    systemcall::utils::{SyscallError, SyscallImpl},
};

const O_NONBLOCK: i32 = 0o4_000;
const O_CLOEXEC: i32 = 0o2_000_000;

fn create_pipe(fds: *mut i32, flags: i32) -> Result<usize, SyscallError> {
    if fds.is_null() {
        return Err(SyscallError::BadAddress);
    }
    if (flags & !(O_NONBLOCK | O_CLOEXEC)) != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let kind = SOCK_STREAM
        | if (flags & O_NONBLOCK) != 0 {
            SOCK_NONBLOCK
        } else {
            0
        };
    let (read_end, write_end) = UnixSocketObject::pair(AF_UNIX, kind, 0)
        .map_err(crate::object::error::ObjectError::from)?;

    read_end
        .shutdown(1)
        .map_err(crate::object::error::ObjectError::from)?;
    write_end
        .shutdown(0)
        .map_err(crate::object::error::ObjectError::from)?;

    let process = get_current_process();
    let (read_fd, write_fd) = {
        let mut process = process.lock();
        let read_fd = process.push_object(read_end);
        let write_fd = process.push_object(write_end);
        (read_fd, write_fd)
    };

    unsafe {
        *fds.add(0) = i32::try_from(read_fd).map_err(|_| SyscallError::TooManyOpenFilesProcess)?;
        *fds.add(1) = i32::try_from(write_fd).map_err(|_| SyscallError::TooManyOpenFilesProcess)?;
    }

    Ok(0)
}

define_syscall!(Pipe, |fds: *mut i32| { create_pipe(fds, 0) });

define_syscall!(Pipe2, |fds: *mut i32, flags: i32| {
    create_pipe(fds, flags)
});
