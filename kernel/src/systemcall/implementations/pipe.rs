use crate::{
    define_syscall,
    memory::user_safe,
    object::{FileFlags, pipe::PipeEndpoint},
    process::{FdFlags, manager::get_current_process},
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

    let object_flags = if flags.contains(PipeFlags::O_NONBLOCK) {
        FileFlags::NONBLOCK
    } else {
        FileFlags::empty()
    };
    let (read_end, write_end) = PipeEndpoint::pair(object_flags);

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
        filesystem::info::LinuxStat,
        memory::user_safe,
        object::FileFlags,
        process::FdFlags,
        systemcall::{
            implementations::{Dup, Dup2, Dup3, Fstat, Ioctl, Pipe, Pipe2, Read, Write},
            test::{
                assert_fd_flags, assert_object_flags, assert_same_object, close_test_fd, expect_fd,
                occupied_fd_count,
            },
            test_helpers::{
                SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, read_user_value,
                write_user_value,
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
        const S_IFMT: u32 = 0o170000;
        const S_IFIFO: u32 = 0o010000;
        const FIONBIO: u64 = 0x5421;
        const FIOCLEX: u64 = 0x5451;

        let fd_page = allocate_user_test_page();
        let stat_page = fd_page + 512;

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
        assert_fd_flags(read_fd, FdFlags::empty());
        assert_fd_flags(write_fd, FdFlags::empty());
        assert_object_flags(read_fd, FileFlags::empty());
        assert_object_flags(write_fd, FileFlags::empty());
        expect_ok(
            SyscallArgs::new([read_fd as u64, stat_page, 0, 0, 0, 0]).call::<Fstat>(),
            0,
        );
        assert_eq!(
            read_user_value::<LinuxStat>(stat_page).st_mode & S_IFMT,
            S_IFIFO,
            "fstat on a pipe fd must report a FIFO, not fail with EBADF"
        );
        write_user_value(fd_page + 640, &1i32);
        expect_ok(
            SyscallArgs::new([read_fd as u64, FIONBIO, fd_page + 640, 0, 0, 0]).call::<Ioctl>(),
            0,
        );
        assert_object_flags(read_fd, FileFlags::NONBLOCK);
        expect_ok(
            SyscallArgs::new([read_fd as u64, FIOCLEX, 0, 0, 0, 0]).call::<Ioctl>(),
            0,
        );
        assert_fd_flags(read_fd, FdFlags::CLOEXEC);
        write_user_value(fd_page + 640, &0i32);
        expect_ok(
            SyscallArgs::new([read_fd as u64, FIONBIO, fd_page + 640, 0, 0, 0]).call::<Ioctl>(),
            0,
        );
        assert_object_flags(read_fd, FileFlags::empty());
        expect_errno(
            SyscallArgs::new([read_fd as u64, FIONBIO, 1, 0, 0, 0]).call::<Ioctl>(),
            SyscallError::BadAddress,
        );

        user_safe::write_buffer(fd_page as *mut u8, b"abc")
            .expect("test buffer should be writable");
        expect_ok(
            SyscallArgs::new([write_fd as u64, fd_page, 3, 0, 0, 0]).call::<Write>(),
            3,
        );
        user_safe::write_buffer(fd_page as *mut u8, &[0; 3])
            .expect("test buffer should be writable");
        expect_ok(
            SyscallArgs::new([read_fd as u64, fd_page, 3, 0, 0, 0]).call::<Read>(),
            3,
        );
        assert_eq!(
            user_safe::read_buffer(fd_page as *const u8, 3).unwrap(),
            b"abc"
        );
        expect_ok(SyscallArgs::new([fd_page, 0, 0, 0, 0, 0]).call::<Pipe>(), 0);
        let sigpipe_fds = read_user_value::<[i32; 2]>(fd_page);
        let sigpipe_read_fd = sigpipe_fds[0] as usize;
        let sigpipe_write_fd = sigpipe_fds[1] as usize;
        let duplicated_read =
            expect_fd(SyscallArgs::new([sigpipe_read_fd as u64, 0, 0, 0, 0, 0]).call::<Dup>());
        expect_ok(
            SyscallArgs::new([sigpipe_write_fd as u64, fd_page, 3, 0, 0, 0]).call::<Write>(),
            3,
        );
        close_test_fd(sigpipe_read_fd);
        expect_ok(
            SyscallArgs::new([sigpipe_write_fd as u64, fd_page, 3, 0, 0, 0]).call::<Write>(),
            3,
        );
        close_test_fd(duplicated_read);
        expect_errno(
            SyscallArgs::new([sigpipe_write_fd as u64, fd_page, 3, 0, 0, 0]).call::<Write>(),
            SyscallError::BrokenPipe,
        );
        close_test_fd(sigpipe_write_fd);

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
            SyscallArgs::new([pipe2_read_fd as u64, i32::MAX as u64, 0, 0, 0, 0]).call::<Dup2>(),
            SyscallError::BadFileDescriptor,
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
            SyscallArgs::new([pipe2_read_fd as u64, i32::MAX as u64, 0, 0, 0, 0]).call::<Dup3>(),
            SyscallError::BadFileDescriptor,
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
        expect_ok(
            SyscallArgs::new([read_fd as u64, fd_page, 1, 0, 0, 0]).call::<Read>(),
            0,
        );
        close_test_fd(read_fd);

        expect_ok(SyscallArgs::new([fd_page, 0, 0, 0, 0, 0]).call::<Pipe>(), 0);
        let pipe_fds = read_user_value::<[i32; 2]>(fd_page);
        let read_fd = pipe_fds[0] as usize;
        let write_fd = pipe_fds[1] as usize;
        close_test_fd(read_fd);
        write_user_value(fd_page, &1u8);
        expect_errno(
            SyscallArgs::new([write_fd as u64, fd_page, 1, 0, 0, 0]).call::<Write>(),
            SyscallError::BrokenPipe,
        );
        close_test_fd(write_fd);
    }
}
