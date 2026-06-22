use super::*;

define_syscall!(Pause, {
    loop {
        match block_current_with_sig_check(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline: None,
        }) {
            Ok(()) => continue,
            Err(err) => return Err(err.as_syscall_error()),
        }
    }
});

define_syscall!(Alarm, |seconds: u32| {
    const ITIMER_REAL: usize = 0;
    const NANOS_PER_SECOND: u64 = 1_000_000_000;

    let process = get_current_process();
    let mut process = process.lock();
    let now = crate::misc::time::Time::since_boot();
    let remaining_seconds = process
        .timers
        .get(ITIMER_REAL)
        .and_then(Option::as_ref)
        .map(|timer| match timer.state {
            crate::misc::timer::TimerState::Disabled => 0,
            crate::misc::timer::TimerState::OneShot { deadline }
            | crate::misc::timer::TimerState::Periodic { deadline, .. } => {
                deadline
                    .sub(now)
                    .as_nanoseconds()
                    .saturating_add(NANOS_PER_SECOND - 1)
                    / NANOS_PER_SECOND
            }
        })
        .unwrap_or(0);

    if process.timers.get(ITIMER_REAL).is_none() {
        process.timers.resize_with(ITIMER_REAL + 1, || None);
    }

    let state = if seconds == 0 {
        crate::misc::timer::TimerState::Disabled
    } else {
        crate::misc::timer::TimerState::OneShot {
            deadline: now.add_ns((seconds as u64).saturating_mul(NANOS_PER_SECOND)),
        }
    };
    process.timers[ITIMER_REAL] = Some(crate::misc::timer::Timer {
        notify_method: crate::misc::timer::TimerNotifyMethod::Signal(
            crate::signal::Signal::SIGALRM,
        ),
        time_type: crate::misc::timer::ClockId::SinceBoot,
        state,
        overrun: 0,
    });

    Ok(remaining_seconds as usize)
});

define_syscall!(RtSigsuspend, |mask: *const u64, sigset_size: usize| {
    if sigset_size != 8 {
        return Err(SyscallError::InvalidArguments);
    }

    if mask.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let new_mask = Signals::from_bits_truncate(user_safe::read(mask)?);
    super::super::select::with_temporary_signal_mask(Some(new_mask), || {
        loop {
            let result = block_current_with_sig_check(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });

            if result.is_err() {
                return Err(SyscallError::Interrupted);
            }
        }
    })
});

define_syscall!(Unshare, |flags: u64| {
    let unsupported = flags
        & !UnshareFlags::all().bits()
        & !(CloneFlags::THREAD | CloneFlags::SIGHAND | CloneFlags::VM | CloneFlags::CLEAR_SIGHAND)
            .bits();
    if unsupported != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let forbidden = flags
        & (CloneFlags::THREAD | CloneFlags::SIGHAND | CloneFlags::VM | CloneFlags::CLEAR_SIGHAND)
            .bits();
    if forbidden != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let supported_namespace_flags = (UnshareFlags::NEWNET | UnshareFlags::NEWNS).bits();
    let unsupported_namespace_flags = (UnshareFlags::NEWCGROUP
        | UnshareFlags::NEWUTS
        | UnshareFlags::NEWIPC
        | UnshareFlags::NEWUSER
        | UnshareFlags::NEWPID)
        .bits();
    if flags & unsupported_namespace_flags != 0 {
        return Err(SyscallError::OperationNotSupported);
    }
    if flags & supported_namespace_flags != 0 {
        let process = get_current_process();
        let mut process = process.lock();
        if flags & UnshareFlags::NEWNET.bits() != 0 {
            process.net_namespace = NetNamespace::new();
        }
        if flags & UnshareFlags::NEWNS.bits() != 0 {
            process.mount_namespace_snapshot = Some(
                crate::filesystem::vfs::VirtualFS
                    .lock()
                    .mount_snapshots()
                    .into_iter()
                    .map(|(_, _, _, _, _, mount_id)| mount_id)
                    .collect(),
            );
        }
    }
    if flags & UnshareFlags::FS.bits() != 0 {
        let process = get_current_process();
        let mut process = process.lock();
        process.fs_context = crate::process::clone_fs_context(&process.fs_context);
    }

    Ok(0)
});

define_syscall!(Setns, |fd: ObjectRef, flags: SetnsFlags| {
    let namespace_object = fd
        .clone()
        .as_file_like()
        .ok()
        .and_then(|file| file.device_backing_object())
        .unwrap_or(fd);
    let net_namespace = namespace_object
        .as_net_namespace()
        .map_err(|_| SyscallError::InvalidArguments)?;
    if !flags.is_empty() && flags != SetnsFlags::NEWNET {
        return Err(SyscallError::InvalidArguments);
    }

    get_current_process().lock().net_namespace = net_namespace;
    Ok(0)
});

define_syscall!(Clone, |flags: u64,
                        stack_pointer: u64,
                        parent_tid: *mut i32,
                        child_tid: *mut i32,
                        tls: u64| {
    let clone_flags = CloneFlags::from_bits_truncate(flags);
    let exit_signal = (flags & 0xff) as u8;
    let required = CloneFlags::VM
        | CloneFlags::FS
        | CloneFlags::FILES
        | CloneFlags::SIGHAND
        | CloneFlags::THREAD;
    if !clone_flags.contains(CloneFlags::THREAD) {
        if clone_flags.contains(CloneFlags::VFORK) && !clone_flags.contains(CloneFlags::VM) {
            return Err(SyscallError::NoSyscall);
        }
        if clone_flags.contains(CloneFlags::VM) && !clone_flags.contains(CloneFlags::VFORK) {
            return Err(SyscallError::NoSyscall);
        }
        let pidfd_ptr = if clone_flags.contains(CloneFlags::PIDFD) {
            parent_tid
        } else {
            core::ptr::null_mut()
        };
        return clone_process(CloneProcessArgs {
            clone_flags,
            raw_flags: flags,
            exit_signal,
            stack_pointer,
            parent_tid,
            child_tid,
            tls,
            pidfd_ptr,
            cgroup_fd: 0,
        });
    }

    let flags = clone_flags;
    if flags.contains(CloneFlags::NEWNET) {
        return Err(SyscallError::InvalidArguments);
    }
    if !flags.contains(required) {
        return Err(SyscallError::NoSyscall);
    }

    with_current_thread(|thread| {
        let process = get_current_process();
        let thread = thread.clone_and_spawn(process.clone());

        {
            let mut child = thread.lock();
            if stack_pointer != 0 {
                child.snapshot.inner.rsp = stack_pointer;
            }
            child.snapshot.inner.rax = 0;
            if flags.contains(CloneFlags::SETTLS) {
                child.snapshot.fs_base = tls;
            }
            if flags.contains(CloneFlags::CHILD_CLEARTID) {
                child.clear_child_tid = child_tid as u64;
            }
        }

        let tid = thread.lock().id.0 as i32;

        if flags.contains(CloneFlags::PARENT_SETTID) {
            user_safe::write(parent_tid, &tid)?;
        }

        if flags.contains(CloneFlags::CHILD_SETTID) {
            user_safe::write(child_tid, &tid)?;
        }

        process.clone().lock().threads.push(Arc::downgrade(&thread));

        Ok(tid as usize)
    })
});

define_syscall!(Vfork, {
    let current = get_current_process();
    let (child_process, child_thread) = Process::vfork(current.clone());
    let pid = child_process.lock().pid;
    MANAGER.lock().processes.insert(pid, child_process.clone());
    child_process.lock().vfork_blocker = Some(crate::thread::get_current_thread().lock().id);

    Process::wake_vfork_child(child_thread);
    wait_for_vfork_completion(&child_process);

    Ok(pid.0 as usize)
});

define_syscall!(Clone3, |args: *const LinuxCloneArgs, size: usize| {
    if size < core::mem::size_of::<LinuxCloneArgs>() {
        return Err(SyscallError::InvalidArguments);
    }
    if args.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let args = user_safe::read(args)?;
    if args.set_tid != 0 || args.set_tid_size != 0 {
        return Err(SyscallError::NoSyscall);
    }

    let stack_pointer = if args.stack == 0 {
        0
    } else {
        args.stack.saturating_add(args.stack_size)
    };
    let flags = args.flags | (args.exit_signal & 0xff);
    let clone_flags = CloneFlags::from_bits_truncate(flags);

    if clone_flags.contains(CloneFlags::THREAD) {
        if clone_flags.contains(CloneFlags::NEWNET) {
            return Err(SyscallError::InvalidArguments);
        }
        return <Clone as SyscallImpl>::handle_call(
            flags,
            stack_pointer,
            args.parent_tid,
            args.child_tid,
            args.tls,
            0,
        );
    }

    if clone_flags.contains(CloneFlags::VFORK) && !clone_flags.contains(CloneFlags::VM) {
        return Err(SyscallError::NoSyscall);
    }
    if clone_flags.contains(CloneFlags::VM) && !clone_flags.contains(CloneFlags::VFORK) {
        return Err(SyscallError::NoSyscall);
    }

    if clone_flags.contains(CloneFlags::PIDFD) != (args.pidfd != 0) {
        return Err(SyscallError::InvalidArguments);
    }
    if clone_flags.contains(CloneFlags::INTO_CGROUP) != (args.cgroup != 0) {
        return Err(SyscallError::InvalidArguments);
    }

    clone_process(CloneProcessArgs {
        clone_flags,
        raw_flags: flags,
        exit_signal: (args.exit_signal & 0xff) as u8,
        stack_pointer,
        parent_tid: args.parent_tid as *mut i32,
        child_tid: args.child_tid as *mut i32,
        tls: args.tls,
        pidfd_ptr: args.pidfd as *mut i32,
        cgroup_fd: args.cgroup,
    })
});

define_syscall!(PidfdOpen, |pid: i32, flags: u32| {
    if pid <= 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if flags != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    get_process_with_pid(ProcessID(pid as u64))?;
    let pidfd: Arc<dyn Object> = PidFdObject::new(pid as u64);
    Ok(get_current_process()
        .lock()
        .push_object_with_flags(pidfd, FdFlags::CLOEXEC))
});

define_syscall!(Kcmp, |pid1: i32,
                       pid2: i32,
                       kind: u32,
                       idx1: usize,
                       idx2: usize| {
    if pid1 <= 0 || pid2 <= 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let current_pid = {
        let process = get_current_process();
        process.lock().pid.0 as i32
    };
    if pid1 != current_pid || pid2 != current_pid {
        return Err(SyscallError::PermissionDenied);
    }

    let kind = KcmpType::try_from(kind).map_err(|_| SyscallError::InvalidArguments)?;
    match kind {
        KcmpType::File => {
            let process = get_process_with_pid(ProcessID(pid1 as u64))?;
            let process = process.lock();
            let object1 = process_fd_object(&process, idx1)?;
            let object2 = process_fd_object(&process, idx2)?;
            if Arc::ptr_eq(&object1, &object2) {
                Ok(0)
            } else {
                let ptr1 = Arc::as_ptr(&object1) as *const () as usize;
                let ptr2 = Arc::as_ptr(&object2) as *const () as usize;
                Ok(match ptr1.cmp(&ptr2) {
                    core::cmp::Ordering::Less => 1,
                    core::cmp::Ordering::Greater => 2,
                    core::cmp::Ordering::Equal => 0,
                })
            }
        }
    }
});

#[cfg(test)]
mod tests {
    use crate::{
        object::misc::get_object_current_process,
        process::{
            FdFlags, Process,
            manager::{MANAGER, get_current_process},
        },
        smp::set_current_process,
        systemcall::{
            implementations::{Clone, Clone3, Eventfd, Kcmp, OpenAt, OpenFlags, Setns, Unshare},
            test::{TestLinuxCloneArgs, close_test_fd, expect_fd, write_user_cstr},
            test_helpers::{
                SyscallArgs, allocate_user_test_page, assert_linux_layout, expect_errno, expect_ok,
                read_user_value, write_user_value,
            },
            utils::SyscallError,
        },
        thread::THREAD_MANAGER,
    };

    crate::test!(
        namespace_and_kcmp_syscalls,
        "namespace and kcmp syscalls follow linux rules",
        namespace_and_kcmp_syscalls_follow_linux_rules
    );
    crate::test!(
        clone_and_fork_syscalls,
        "clone fork and clone3 syscalls follow linux rules",
        clone_and_fork_syscalls_follow_linux_rules
    );

    fn namespace_and_kcmp_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const CLONE_NEWNET: u64 = 0x4000_0000;

        let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let saved_namespace = get_current_process().lock().net_namespace.clone();
        let proc_ns = allocate_user_test_page();
        write_user_cstr(proc_ns, b"/proc/self/ns/net\0");
        let ns_fd = expect_fd(
            SyscallArgs::new([AT_FDCWD, proc_ns, OpenFlags::empty().bits() as u64, 0, 0, 0])
                .call::<OpenAt>(),
        );
        let original_inode = saved_namespace.inode();
        expect_ok(
            SyscallArgs::new([CLONE_NEWNET, 0, 0, 0, 0, 0]).call::<Unshare>(),
            0,
        );
        let new_inode = get_current_process().lock().net_namespace.inode();
        assert_ne!(new_inode, original_inode);
        expect_ok(
            SyscallArgs::new([ns_fd as u64, 0, 0, 0, 0, 0]).call::<Setns>(),
            0,
        );
        assert_eq!(
            get_current_process().lock().net_namespace.inode(),
            original_inode
        );
        expect_ok(
            SyscallArgs::new([ns_fd as u64, CLONE_NEWNET, 0, 0, 0, 0]).call::<Setns>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([eventfd as u64, 0, 0, 0, 0, 0]).call::<Setns>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([ns_fd as u64, 0x2000_0000, 0, 0, 0, 0]).call::<Setns>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0x8000_0000, 0, 0, 0, 0, 0]).call::<Unshare>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0x20000, 0, 0, 0, 0, 0]).call::<Unshare>(),
            SyscallError::OperationNotSupported,
        );

        let saved_process = get_current_process();
        let other_eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let kcmp_process = Process::init();
        let kcmp_pid = kcmp_process.lock().pid.0 as u64;
        MANAGER.lock().processes.insert(
            crate::process::misc::ProcessID(kcmp_pid),
            kcmp_process.clone(),
        );
        let same_object;
        let other_kcmp_fd;
        {
            let mut process = kcmp_process.lock();
            same_object = (
                process.push_object_with_flags(
                    get_object_current_process(eventfd as u64).unwrap(),
                    FdFlags::empty(),
                ),
                process.push_object_with_flags(
                    get_object_current_process(eventfd as u64).unwrap(),
                    FdFlags::empty(),
                ),
            );
            other_kcmp_fd = process.push_object_with_flags(
                get_object_current_process(other_eventfd as u64).unwrap(),
                FdFlags::empty(),
            );
        }
        set_current_process(Some(kcmp_process.clone()));
        let kcmp_equal = SyscallArgs::new([
            kcmp_pid,
            kcmp_pid,
            0,
            same_object.0 as u64,
            same_object.1 as u64,
            0,
        ])
        .call::<Kcmp>();
        expect_ok(kcmp_equal, 0);
        let cmp_ab = SyscallArgs::new([
            kcmp_pid,
            kcmp_pid,
            0,
            same_object.0 as u64,
            other_kcmp_fd as u64,
            0,
        ])
        .call::<Kcmp>()
        .expect("kcmp should compare file objects");
        let cmp_ba = SyscallArgs::new([
            kcmp_pid,
            kcmp_pid,
            0,
            other_kcmp_fd as u64,
            same_object.0 as u64,
            0,
        ])
        .call::<Kcmp>()
        .expect("kcmp reverse should compare file objects");
        assert!(matches!((cmp_ab, cmp_ba), (1, 2) | (2, 1)));
        expect_errno(
            SyscallArgs::new([
                0,
                kcmp_pid,
                0,
                same_object.0 as u64,
                other_kcmp_fd as u64,
                0,
            ])
            .call::<Kcmp>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                kcmp_pid + 1,
                kcmp_pid,
                0,
                same_object.0 as u64,
                other_kcmp_fd as u64,
                0,
            ])
            .call::<Kcmp>(),
            SyscallError::PermissionDenied,
        );
        expect_errno(
            SyscallArgs::new([
                kcmp_pid,
                kcmp_pid,
                99,
                same_object.0 as u64,
                other_kcmp_fd as u64,
                0,
            ])
            .call::<Kcmp>(),
            SyscallError::InvalidArguments,
        );
        set_current_process(Some(saved_process));
        MANAGER
            .lock()
            .processes
            .remove(&crate::process::misc::ProcessID(kcmp_pid));

        close_test_fd(other_eventfd);
        close_test_fd(ns_fd);
        close_test_fd(eventfd);
        get_current_process().lock().net_namespace = saved_namespace;
    }
    fn clone_and_fork_syscalls_follow_linux_rules() {
        const SIGCHLD: u64 = 17;
        const CLONE_VM: u64 = 0x0000_0100;
        const CLONE_FS: u64 = 0x0000_0200;
        const CLONE_FILES: u64 = 0x0000_0400;
        const CLONE_SIGHAND: u64 = 0x0000_0800;
        const CLONE_PIDFD: u64 = 0x0000_1000;
        const CLONE_VFORK: u64 = 0x0000_4000;
        const CLONE_THREAD: u64 = 0x0001_0000;
        const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
        const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
        const CLONE_CHILD_SETTID: u64 = 0x0100_0000;

        assert_linux_layout::<TestLinuxCloneArgs>(88, 8);

        let page = allocate_user_test_page();

        write_user_value(page, &0i32);
        write_user_value(page + 8, &0i32);
        expect_errno(
            SyscallArgs::new([
                CLONE_PIDFD
                    | CLONE_PARENT_SETTID
                    | CLONE_CHILD_SETTID
                    | CLONE_CHILD_CLEARTID
                    | SIGCHLD,
                0,
                page,
                page + 8,
                0,
                0,
            ])
            .call::<Clone>(),
            SyscallError::NoSyscall,
        );

        expect_errno(
            SyscallArgs::new([CLONE_VFORK | SIGCHLD, 0, 0, 0, 0, 0]).call::<Clone>(),
            SyscallError::NoSyscall,
        );
        expect_errno(
            SyscallArgs::new([CLONE_VM | SIGCHLD, 0, 0, 0, 0, 0]).call::<Clone>(),
            SyscallError::NoSyscall,
        );
        expect_errno(
            SyscallArgs::new([CLONE_PIDFD | SIGCHLD, 0, 0, 0, 0, 0]).call::<Clone>(),
            SyscallError::BadAddress,
        );

        expect_errno(
            SyscallArgs::new([
                CLONE_THREAD | CLONE_VM | CLONE_FS | CLONE_FILES,
                0,
                0,
                0,
                0,
                0,
            ])
            .call::<Clone>(),
            SyscallError::NoSyscall,
        );

        write_user_value(
            page + 256,
            &TestLinuxCloneArgs {
                set_tid: 1,
                ..Default::default()
            },
        );
        expect_errno(
            SyscallArgs::new([
                page + 256,
                core::mem::size_of::<TestLinuxCloneArgs>() as u64,
                0,
                0,
                0,
                0,
            ])
            .call::<Clone3>(),
            SyscallError::NoSyscall,
        );
        expect_errno(
            SyscallArgs::new([
                0,
                core::mem::size_of::<TestLinuxCloneArgs>() as u64,
                0,
                0,
                0,
                0,
            ])
            .call::<Clone3>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([page + 256, 8, 0, 0, 0, 0]).call::<Clone3>(),
            SyscallError::InvalidArguments,
        );

        write_user_value(
            page + 288,
            &TestLinuxCloneArgs {
                flags: CLONE_PIDFD | CLONE_PARENT_SETTID,
                pidfd: page + 320,
                parent_tid: page + 320,
                exit_signal: SIGCHLD,
                ..Default::default()
            },
        );
        expect_errno(
            SyscallArgs::new([
                page + 288,
                core::mem::size_of::<TestLinuxCloneArgs>() as u64,
                0,
                0,
                0,
                0,
            ])
            .call::<Clone3>(),
            SyscallError::NoSyscall,
        );

        write_user_value(
            page + 384,
            &TestLinuxCloneArgs {
                flags: CLONE_PIDFD,
                exit_signal: SIGCHLD,
                ..Default::default()
            },
        );
        expect_errno(
            SyscallArgs::new([
                page + 384,
                core::mem::size_of::<TestLinuxCloneArgs>() as u64,
                0,
                0,
                0,
                0,
            ])
            .call::<Clone3>(),
            SyscallError::InvalidArguments,
        );

        write_user_value(page + 416, &0i32);
        let thread_tid = SyscallArgs::new([
            CLONE_THREAD | CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_CHILD_CLEARTID,
            0,
            0,
            page + 416,
            0,
            0,
        ])
        .call::<Clone>()
        .expect("clone thread with child_cleartid should succeed");
        assert_eq!(
            read_user_value::<i32>(page + 416),
            0,
            "CLONE_CHILD_CLEARTID must not write the child tid at creation time"
        );
        assert!(thread_tid > 0);

        let process = get_current_process();
        let spawned = {
            let process = process.lock();
            process
                .threads
                .iter()
                .filter_map(|thread| thread.upgrade())
                .find(|thread| thread.lock().id.0 == thread_tid as u64)
                .expect("spawned clone thread should be registered")
        };
        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
        thread_manager.mark_thread_exited(spawned);
        thread_manager.cleanup_exited_threads();
    }
}
