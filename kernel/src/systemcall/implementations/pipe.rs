use crate::{
    define_syscall,
    memory::user_safe,
    object::error::ObjectError,
    process::{FdFlags, manager::get_current_process},
    socket::{AF_UNIX, SOCK_NONBLOCK, SOCK_STREAM, UnixSocketObject},
    systemcall::utils::{SyscallError, SyscallImpl},
};
use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct PipeFlags: i32 {
        const O_NONBLOCK = 0o4_000;
        const O_CLOEXEC = 0o2_000_000;
    }
}

fn create_pipe(fds: *mut i32, flags: i32) -> Result<usize, SyscallError> {
    let flags = PipeFlags::from_bits(flags).ok_or(SyscallError::InvalidArguments)?;

    let kind = SOCK_STREAM
        | if flags.contains(PipeFlags::O_NONBLOCK) {
            SOCK_NONBLOCK
        } else {
            0
        };
    let (read_end, write_end) =
        UnixSocketObject::pair(AF_UNIX, kind, 0).map_err(ObjectError::from)?;

    read_end.shutdown(1).map_err(ObjectError::from)?;
    write_end.shutdown(0).map_err(ObjectError::from)?;

    let process = get_current_process();
    let (read_fd, write_fd) = {
        let mut process = process.lock();
        let fd_flags = if flags.contains(PipeFlags::O_CLOEXEC) {
            FdFlags::CLOEXEC
        } else {
            FdFlags::empty()
        };
        let read_fd = process.push_object_with_flags(read_end, fd_flags);
        let write_fd = process.push_object_with_flags(write_end, fd_flags);
        (read_fd, write_fd)
    };

    let fds_out = [
        i32::try_from(read_fd).map_err(|_| SyscallError::TooManyOpenFilesProcess)?,
        i32::try_from(write_fd).map_err(|_| SyscallError::TooManyOpenFilesProcess)?,
    ];
    user_safe::write(fds, &fds_out)?;

    Ok(0)
}

define_syscall!(Pipe, |fds: *mut i32| { create_pipe(fds, 0) });

define_syscall!(Pipe2, |fds: *mut i32, flags: i32| {
    create_pipe(fds, flags)
});
