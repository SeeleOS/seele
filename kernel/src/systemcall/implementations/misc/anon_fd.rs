use super::*;

define_syscall!(InotifyInit, {
    let object = Arc::new(InotifyObject::default());
    let fd = get_current_process().lock().push_object(object);
    Ok(fd)
});

define_syscall!(InotifyInit1, |flags: InotifyInitFlags| {
    let object = Arc::new(InotifyObject::default());
    if flags.contains(InotifyInitFlags::IN_NONBLOCK) {
        let _ = object.clone().set_flags(FileFlags::NONBLOCK);
    }
    let fd_flags = if flags.contains(InotifyInitFlags::IN_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(object, fd_flags);
    Ok(fd)
});

fn create_eventfd(initval: u32, flags: EventFdFlags) -> Result<usize, SyscallError> {
    if flags.bits() & !EventFdFlags::all().bits() != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let object = EventFdObject::new(initval as u64, flags);
    let fd_flags = if flags.contains(EventFdFlags::EFD_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(object, fd_flags);
    Ok(fd)
}

define_syscall!(Eventfd, |initval: u32| {
    create_eventfd(initval, EventFdFlags::empty())
});

define_syscall!(Eventfd2, |initval: u32, flags: EventFdFlags| {
    create_eventfd(initval, flags)
});

define_syscall!(
    InotifyAddWatch,
    |object: ObjectRef, _path: String, _mask: u32| {
        Ok(object.as_inotify()?.add_watch() as usize)
    }
);

define_syscall!(InotifyRmWatch, |object: ObjectRef, _wd: i32| {
    let _ = object.as_inotify()?;
    Ok(0)
});

#[cfg(test)]
mod tests {
    use crate::{
        object::{FileFlags, misc::get_object_current_process},
        process::FdFlags,
        systemcall::{
            implementations::{
                Eventfd, Eventfd2, Fcntl, InotifyAddWatch, InotifyInit, InotifyInit1,
                InotifyRmWatch, MemfdCreate,
            },
            test::{
                assert_fd_flags, assert_object_flags, close_test_fd, expect_fd, write_user_cstr,
            },
            test_helpers::{SyscallArgs, allocate_user_test_page, expect_errno, expect_ok},
            utils::SyscallError,
        },
    };

    crate::test!(
        eventfd_syscalls,
        "eventfd syscalls follow linux flag rules",
        eventfd_syscalls_follow_linux_flag_rules
    );
    crate::test!(
        inotify_init_syscalls,
        "inotify init syscalls follow linux flag rules",
        inotify_init_syscalls_follow_linux_flag_rules
    );

    crate::test!(
        memfd_and_inotify_watch_syscalls,
        "memfd and inotify watch syscalls follow linux rules",
        memfd_and_inotify_watch_syscalls_follow_linux_rules
    );

    fn eventfd_syscalls_follow_linux_flag_rules() {
        const EFD_SEMAPHORE: u64 = 0x1;
        const EFD_NONBLOCK: u64 = 0o4_000;
        const EFD_CLOEXEC: u64 = 0o2_000_000;

        let eventfd = expect_fd(SyscallArgs::new([7, 0, 0, 0, 0, 0]).call::<Eventfd>());
        assert!(
            get_object_current_process(eventfd as u64)
                .expect("eventfd should resolve")
                .as_eventfd()
                .is_ok()
        );
        assert_fd_flags(eventfd, FdFlags::empty());
        assert_object_flags(eventfd, FileFlags::empty());
        close_test_fd(eventfd);

        let eventfd2 = expect_fd(
            SyscallArgs::new([0, EFD_SEMAPHORE | EFD_NONBLOCK | EFD_CLOEXEC, 0, 0, 0, 0])
                .call::<Eventfd2>(),
        );
        assert_fd_flags(eventfd2, FdFlags::CLOEXEC);
        assert_object_flags(eventfd2, FileFlags::NONBLOCK);
        close_test_fd(eventfd2);
        expect_errno(
            SyscallArgs::new([0, 0x8000_0000, 0, 0, 0, 0]).call::<Eventfd2>(),
            SyscallError::InvalidArguments,
        );
    }

    fn inotify_init_syscalls_follow_linux_flag_rules() {
        const IN_NONBLOCK: u64 = 0o4_000;
        const IN_CLOEXEC: u64 = 0o2_000_000;

        let inotify = expect_fd(SyscallArgs::none().call::<InotifyInit>());
        assert!(
            get_object_current_process(inotify as u64)
                .expect("inotify fd should resolve")
                .as_inotify()
                .is_ok()
        );
        assert_fd_flags(inotify, FdFlags::empty());
        assert_object_flags(inotify, FileFlags::empty());
        close_test_fd(inotify);

        let inotify1 = expect_fd(
            SyscallArgs::new([IN_NONBLOCK | IN_CLOEXEC, 0, 0, 0, 0, 0]).call::<InotifyInit1>(),
        );
        assert_fd_flags(inotify1, FdFlags::CLOEXEC);
        assert_object_flags(inotify1, FileFlags::NONBLOCK);
        close_test_fd(inotify1);
        expect_errno(
            SyscallArgs::new([0x8000_0000, 0, 0, 0, 0, 0]).call::<InotifyInit1>(),
            SyscallError::InvalidArguments,
        );
    }

    fn memfd_and_inotify_watch_syscalls_follow_linux_rules() {
        const MFD_CLOEXEC: u64 = 0x0001;
        const MFD_ALLOW_SEALING: u64 = 0x0002;
        const MFD_NOEXEC_SEAL: u64 = 0x0008;
        const MFD_EXEC: u64 = 0x0010;

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"demo/memfd\0");
        let memfd = expect_fd(
            SyscallArgs::new([user_page, MFD_CLOEXEC | MFD_ALLOW_SEALING, 0, 0, 0, 0])
                .call::<MemfdCreate>(),
        );
        assert_fd_flags(memfd, FdFlags::CLOEXEC);
        let memfd_stat = get_object_current_process(memfd as u64)
            .unwrap()
            .as_statable()
            .unwrap()
            .stat();
        assert_eq!(memfd_stat.st_mode & 0o170000, 0o100000);
        assert_eq!(memfd_stat.st_mode & 0o777, 0o600);

        expect_ok(
            SyscallArgs::new([memfd as u64, 1034, 0, 0, 0, 0]).call::<Fcntl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([memfd as u64, 1033, 0x0002, 0, 0, 0]).call::<Fcntl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([memfd as u64, 1034, 0, 0, 0, 0]).call::<Fcntl>(),
            0x0002,
        );

        expect_errno(
            SyscallArgs::new([user_page, 0x4, 0, 0, 0, 0]).call::<MemfdCreate>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([user_page, MFD_NOEXEC_SEAL | MFD_EXEC, 0, 0, 0, 0])
                .call::<MemfdCreate>(),
            SyscallError::InvalidArguments,
        );

        let inotify = expect_fd(SyscallArgs::none().call::<InotifyInit>());
        write_user_cstr(user_page + 128, b"/tmp\0");
        let wd1 = SyscallArgs::new([inotify as u64, user_page + 128, 0xffff_ffff, 0, 0, 0])
            .call::<InotifyAddWatch>()
            .expect("inotify_add_watch should succeed");
        let wd2 = SyscallArgs::new([inotify as u64, user_page + 128, 0, 0, 0, 0])
            .call::<InotifyAddWatch>()
            .expect("second watch should succeed");
        assert!(wd2 > wd1);
        expect_ok(
            SyscallArgs::new([inotify as u64, wd1 as u64, 0, 0, 0, 0]).call::<InotifyRmWatch>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([inotify as u64, wd2 as u64, 0, 0, 0, 0]).call::<InotifyRmWatch>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([memfd as u64, user_page + 128, 0, 0, 0, 0]).call::<InotifyAddWatch>(),
            SyscallError::BadFileDescriptor,
        );
        expect_errno(
            SyscallArgs::new([memfd as u64, 1, 0, 0, 0, 0]).call::<InotifyRmWatch>(),
            SyscallError::BadFileDescriptor,
        );

        close_test_fd(inotify);
        close_test_fd(memfd);
    }
}
