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
            implementations::{Eventfd, Eventfd2, InotifyInit, InotifyInit1},
            test::{assert_fd_flags, assert_object_flags, close_test_fd, expect_fd},
            test_helpers::{SyscallArgs, expect_errno},
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
}
