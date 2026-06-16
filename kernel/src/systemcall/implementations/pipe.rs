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
    pub(crate) struct PipeFlags: i32 {
        const O_NONBLOCK = 0o4_000;
        const O_CLOEXEC = 0o2_000_000;
    }
}

fn create_pipe(fds: *mut i32, flags: PipeFlags) -> Result<usize, SyscallError> {
    if fds.is_null() {
        return Err(SyscallError::BadAddress);
    }

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

define_syscall!(Pipe, |fds: *mut i32| {
    create_pipe(fds, PipeFlags::empty())
});

define_syscall!(Pipe2, |fds: *mut i32, flags: PipeFlags| {
    create_pipe(fds, flags)
});

#[cfg(test)]
mod tests {
    use crate::{
        object::{FileFlags, misc::get_object_current_process},
        process::FdFlags,
        systemcall::{
            implementations::{Dup, Dup2, Dup3, Pipe, Pipe2},
            test::{
                assert_fd_flags, assert_object_flags, assert_same_object, close_test_fd, expect_fd,
                occupied_fd_count,
            },
            test_helpers::{
                SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, read_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        pipe_and_dup_syscalls,
        "pipe and dup syscalls follow linux fd rules",
        pipe_and_dup_syscalls_follow_linux_fd_rules
    );

    fn pipe_and_dup_syscalls_follow_linux_fd_rules() {
        const O_NONBLOCK: u64 = 0o4_000;
        const O_CLOEXEC: u64 = 0o2_000_000;

        let fd_page = allocate_user_test_page();

        let occupied_before_bad_pipe = occupied_fd_count();
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Pipe>(),
            SyscallError::BadAddress,
        );
        assert_eq!(occupied_fd_count(), occupied_before_bad_pipe);

        expect_ok(SyscallArgs::new([fd_page, 0, 0, 0, 0, 0]).call::<Pipe>(), 0);
        let pipe_fds = read_user_value::<[i32; 2]>(fd_page);
        let read_fd = pipe_fds[0] as usize;
        let write_fd = pipe_fds[1] as usize;
        assert_ne!(read_fd, write_fd);
        assert!(
            get_object_current_process(read_fd as u64)
                .expect("pipe read fd should resolve")
                .as_unix_socket()
                .is_ok()
        );
        assert!(
            get_object_current_process(write_fd as u64)
                .expect("pipe write fd should resolve")
                .as_unix_socket()
                .is_ok()
        );
        assert_fd_flags(read_fd, FdFlags::empty());
        assert_fd_flags(write_fd, FdFlags::empty());
        assert_object_flags(read_fd, FileFlags::empty());
        assert_object_flags(write_fd, FileFlags::empty());

        expect_ok(
            SyscallArgs::new([fd_page, O_NONBLOCK | O_CLOEXEC, 0, 0, 0, 0]).call::<Pipe2>(),
            0,
        );
        let pipe2_fds = read_user_value::<[i32; 2]>(fd_page);
        let pipe2_read_fd = pipe2_fds[0] as usize;
        let pipe2_write_fd = pipe2_fds[1] as usize;
        assert_ne!(pipe2_read_fd, pipe2_write_fd);
        assert_fd_flags(pipe2_read_fd, FdFlags::CLOEXEC);
        assert_fd_flags(pipe2_write_fd, FdFlags::CLOEXEC);
        assert_object_flags(pipe2_read_fd, FileFlags::NONBLOCK);
        assert_object_flags(pipe2_write_fd, FileFlags::NONBLOCK);
        expect_errno(
            SyscallArgs::new([fd_page, 0x8000_0000, 0, 0, 0, 0]).call::<Pipe2>(),
            SyscallError::InvalidArguments,
        );

        let dup_fd =
            expect_fd(SyscallArgs::new([pipe2_read_fd as u64, 0, 0, 0, 0, 0]).call::<Dup>());
        assert_same_object(pipe2_read_fd, dup_fd);
        assert_fd_flags(dup_fd, FdFlags::empty());

        let dup2_dest = dup_fd + 5;
        expect_ok(
            SyscallArgs::new([pipe2_read_fd as u64, dup2_dest as u64, 0, 0, 0, 0]).call::<Dup2>(),
            dup2_dest,
        );
        assert_same_object(pipe2_read_fd, dup2_dest);
        assert_fd_flags(dup2_dest, FdFlags::empty());
        expect_ok(
            SyscallArgs::new([pipe2_read_fd as u64, pipe2_read_fd as u64, 0, 0, 0, 0])
                .call::<Dup2>(),
            pipe2_read_fd,
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, u64::MAX, 0, 0, 0, 0]).call::<Dup2>(),
            SyscallError::BadFileDescriptor,
        );

        let dup3_dest = dup2_dest + 1;
        expect_ok(
            SyscallArgs::new([pipe2_read_fd as u64, dup3_dest as u64, O_CLOEXEC, 0, 0, 0])
                .call::<Dup3>(),
            dup3_dest,
        );
        assert_same_object(pipe2_read_fd, dup3_dest);
        assert_fd_flags(dup3_dest, FdFlags::CLOEXEC);
        expect_errno(
            SyscallArgs::new([pipe2_read_fd as u64, pipe2_read_fd as u64, 0, 0, 0, 0])
                .call::<Dup3>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                pipe2_read_fd as u64,
                (dup3_dest + 1) as u64,
                O_NONBLOCK,
                0,
                0,
                0,
            ])
            .call::<Dup3>(),
            SyscallError::InvalidArguments,
        );

        close_test_fd(dup3_dest);
        close_test_fd(dup2_dest);
        close_test_fd(dup_fd);
        close_test_fd(pipe2_write_fd);
        close_test_fd(pipe2_read_fd);
        close_test_fd(write_fd);
        close_test_fd(read_fd);
    }
}
