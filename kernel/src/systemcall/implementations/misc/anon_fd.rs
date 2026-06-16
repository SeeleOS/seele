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
    use crate::systemcall::test::*;

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
}
