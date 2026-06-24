use alloc::{string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;
use strum::IntoEnumIterator;

use crate::{
    define_syscall,
    filesystem::{
        absolute_path::AbsolutePath,
        path::Path,
        vfs_operations::{open_path, open_path_nofollow},
        vfs_traits::FileLikeType,
    },
    memory::user_safe,
    misc::{
        c_types::{CString, CVec},
        others::KernelFrom,
        signal::{SigInfo, SignalHandlingType},
    },
    object::misc::get_object_current_process,
    object::traits::Statable,
    process::{
        Process, ProcessExitStatus, ProcessRef,
        execve::execve,
        manager::{MANAGER, exit_current_thread, get_current_process, terminate_process},
        misc::{ProcessID, get_process_with_pid},
        wait::{ProcessWaitEvent, take_wait_event},
    },
    signal::{Signal, Signals},
    systemcall::utils::{SyscallError, SyscallImpl},
    thread::{
        get_current_thread,
        scheduling::return_to_scheduler_no_save,
        yielding::{
            BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        },
    },
};

use super::filesystem::{
    AtFlags, check_access_path_search_permissions, check_access_permissions_for_ids_with_options,
    fs_access_credentials,
};

const SA_RESTART: u64 = 0x1000_0000;
const AT_FDCWD: i32 = -100;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Wait4Options: i32 {
        const NOHANG = 1;
        const WUNTRACED = 2;
        const WCONTINUED = 8;
        const __WNOTHREAD = 0x2000_0000;
        const __WALL = 0x4000_0000;
        const __WCLONE = i32::MIN;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct WaitidOptions: i32 {
        const NOHANG = 1;
        const WSTOPPED = 2;
        const WEXITED = 4;
        const WCONTINUED = 8;
        const WNOWAIT = 0x0100_0000;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxTimeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxRusage {
    ru_utime: LinuxTimeval,
    ru_stime: LinuxTimeval,
    ru_maxrss: i64,
    ru_ixrss: i64,
    ru_idrss: i64,
    ru_isrss: i64,
    ru_minflt: i64,
    ru_majflt: i64,
    ru_nswap: i64,
    ru_inblock: i64,
    ru_oublock: i64,
    ru_msgsnd: i64,
    ru_msgrcv: i64,
    ru_nsignals: i64,
    ru_nvcsw: i64,
    ru_nivcsw: i64,
}

fn has_wait_interrupt_signal(process: &ProcessRef) -> bool {
    let process = process.lock();
    let current_thread = get_current_thread();
    let current_thread = current_thread.lock();

    let signal_interrupts_wait = |signal: Signal| {
        let action = &process.signal_actions[signal.index()];
        signal != Signal::SIGCHLD
            && matches!(
                action.handling_type,
                SignalHandlingType::Function1(_) | SignalHandlingType::Function2(_)
            )
            && action.flags & SA_RESTART == 0
    };

    Signal::iter().any(|signal| {
        (process.pending_signals.contains(Signals::from(signal))
            || current_thread
                .pending_signals
                .contains(Signals::from(signal)))
            && signal_interrupts_wait(signal)
    })
}

fn consume_ignored_child_signal(process: &ProcessRef) {
    process
        .lock()
        .pending_signals
        .remove(Signal::SIGCHLD.into());
}

const CLD_TRAPPED: i32 = 4;
const CLD_STOPPED: i32 = 5;

enum WaitOutcome {
    Exited(ProcessRef, u64, ProcessExitStatus),
    Stopped(ProcessRef, u64, ProcessWaitEvent),
}

#[derive(Clone, Copy)]
struct WaitBehavior {
    nohang: bool,
    preserve_child: bool,
    report_exited: bool,
    report_stopped: bool,
}

impl WaitBehavior {
    fn for_wait4(options: Wait4Options) -> Self {
        Self {
            nohang: options.contains(Wait4Options::NOHANG),
            preserve_child: false,
            report_exited: true,
            report_stopped: options.contains(Wait4Options::WUNTRACED),
        }
    }

    fn for_waitid(options: WaitidOptions) -> Self {
        Self {
            nohang: options.contains(WaitidOptions::NOHANG),
            preserve_child: options.contains(WaitidOptions::WNOWAIT),
            report_exited: options.contains(WaitidOptions::WEXITED),
            report_stopped: options.contains(WaitidOptions::WSTOPPED),
        }
    }
}

fn check_wait_outcome(
    target_process: i32,
    wait_behavior: WaitBehavior,
    current_process: &ProcessRef,
) -> Result<Option<WaitOutcome>, SyscallError> {
    let current_group = current_process.lock().group_id;
    let manager = MANAGER.lock();
    let mut matched_child = false;
    let mut ready_child = None;
    if target_process == i32::MIN {
        return Err(SyscallError::NoProcess);
    }
    let target_group = match target_process {
        -1..=i32::MAX => None,
        ..=-2 => Some(target_process.wrapping_neg() as u64),
    };

    for (pid, process) in manager.processes.iter() {
        let mut p_lock = process.lock();
        let is_current_child = p_lock
            .parent
            .clone()
            .is_some_and(|parent| Arc::ptr_eq(&parent, current_process));

        let matches = match target_process {
            -1 => is_current_child,
            0 => is_current_child && p_lock.group_id == current_group,
            1.. => pid.0 == target_process as u64 && is_current_child,
            ..=-2 => is_current_child && Some(p_lock.group_id.0) == target_group,
        };

        if !matches {
            continue;
        }

        matched_child = true;

        if wait_behavior.report_exited
            && let Some(exit_status) = p_lock.exit_status
            && p_lock.threads.is_empty()
        {
            ready_child = Some(WaitOutcome::Exited(process.clone(), pid.0, exit_status));
            break;
        }

        if wait_behavior.report_stopped
            && let Some(wait_event) = take_wait_event(&mut p_lock, wait_behavior.preserve_child)
        {
            ready_child = Some(WaitOutcome::Stopped(process.clone(), pid.0, wait_event));
            break;
        }
    }

    if let Some(process) = ready_child {
        Ok(Some(process))
    } else if matched_child {
        Ok(None)
    } else {
        Err(SyscallError::NoChildProcesses)
    }
}

fn wait_for_child_exit(
    target_process: i32,
    wait_behavior: WaitBehavior,
) -> Result<Option<WaitOutcome>, SyscallError> {
    let current_process = get_current_process();

    loop {
        crate::thread::with_thread_manager(|manager| manager.cleanup_exited_threads());

        let check_result = check_wait_outcome(target_process, wait_behavior, &current_process)?;

        match check_result {
            Some(WaitOutcome::Exited(process, pid, exit_status)) => {
                consume_ignored_child_signal(&current_process);
                if !wait_behavior.preserve_child {
                    MANAGER.lock().reap_process(process.clone());
                }
                return Ok(Some(WaitOutcome::Exited(process, pid, exit_status)));
            }
            Some(WaitOutcome::Stopped(process, pid, wait_event)) => {
                consume_ignored_child_signal(&current_process);
                return Ok(Some(WaitOutcome::Stopped(process, pid, wait_event)));
            }
            None if wait_behavior.nohang => return Ok(None),
            None => {
                if has_wait_interrupt_signal(&current_process) {
                    return Err(SyscallError::Interrupted);
                }

                let current = prepare_block_current(BlockType::WakeRequired {
                    wake_type: WakeType::ProcsesExit,
                    deadline: None,
                });
                match check_wait_outcome(target_process, wait_behavior, &current_process) {
                    Ok(Some(outcome)) => {
                        consume_ignored_child_signal(&current_process);
                        cancel_block(&current);
                        finish_block_current();
                        return Ok(Some(outcome));
                    }
                    Err(SyscallError::NoChildProcesses) => {
                        cancel_block(&current);
                        finish_block_current();
                        return Err(SyscallError::NoChildProcesses);
                    }
                    Ok(None) => {}
                    Err(err) => {
                        cancel_block(&current);
                        finish_block_current();
                        return Err(err);
                    }
                }
                finish_block_current();

                if has_wait_interrupt_signal(&current_process) {
                    return Err(SyscallError::Interrupted);
                }
            }
        }
    }
}

define_syscall!(Getppid, {
    if let Some(parent) = get_current_process().lock().parent.clone() {
        Ok(parent.lock().pid.0 as usize)
    } else {
        Ok(0)
    }
});

define_syscall!(Getpgrp, {
    Ok(get_current_process().lock().group_id.0 as usize)
});

define_syscall!(Wait4, |target_process: i32,
                        status_ptr: *mut i32,
                        options: Wait4Options,
                        rusage: *mut LinuxRusage| {
    let wait_behavior = WaitBehavior::for_wait4(options);
    let Some(outcome) = wait_for_child_exit(target_process, wait_behavior)? else {
        return Ok(0);
    };

    if !status_ptr.is_null() {
        let status = match outcome {
            WaitOutcome::Exited(_, _, exit_status) => exit_status.wait_status(),
            WaitOutcome::Stopped(_, _, wait_event) => wait_event.wait_status(),
        };
        user_safe::write(status_ptr, &status)?;
    }
    if !rusage.is_null() {
        user_safe::write(rusage, &LinuxRusage::default())?;
    }

    let pid = match outcome {
        WaitOutcome::Exited(_, pid, _) | WaitOutcome::Stopped(_, pid, _) => pid,
    };
    Ok(pid as usize)
});

define_syscall!(Waitid, |id_type: i32,
                         id: u32,
                         info_ptr: *mut SigInfo,
                         options: WaitidOptions| {
    let target_process = match id_type {
        0 => -1,
        1 => id as i32,
        2 => -(id as i32),
        3 => get_object_current_process(id as u64)?.as_pidfd()?.pid() as i32,
        _ => return Err(SyscallError::InvalidArguments),
    };

    if !options.contains(WaitidOptions::WEXITED) {
        return Err(SyscallError::InvalidArguments);
    }

    let wait_behavior = WaitBehavior::for_waitid(options);
    let result = wait_for_child_exit(target_process, wait_behavior)?;

    if !info_ptr.is_null() {
        let info = if let Some(result) = &result {
            match result {
                WaitOutcome::Exited(_, pid, exit_status) => SigInfo::for_waitid(
                    Signal::SIGCHLD,
                    exit_status.waitid_code(),
                    *pid as i32,
                    exit_status.waitid_status(),
                ),
                WaitOutcome::Stopped(_, pid, wait_event) => SigInfo::for_waitid(
                    Signal::SIGCHLD,
                    if wait_event.is_ptrace() {
                        CLD_TRAPPED
                    } else {
                        CLD_STOPPED
                    },
                    *pid as i32,
                    (wait_event.wait_status() >> 8) & 0xff,
                ),
            }
        } else {
            SigInfo::default()
        };
        user_safe::write(info_ptr, &info)?;
    }

    Ok(0)
});

fn execve_path_from_raw(path: CString) -> Result<String, SyscallError> {
    const PATH_MAX: usize = 4096;
    const NAME_MAX: usize = 255;

    if path.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut out = String::new();
    let mut component_len = 0;
    for index in 0..=PATH_MAX {
        let byte =
            user_safe::read(unsafe { path.add(index) }).map_err(|_| SyscallError::BadAddress)?;
        if byte == 0 {
            return Ok(out);
        }
        if index == PATH_MAX {
            return Err(SyscallError::PathTooLong);
        }
        if byte == b'/' {
            component_len = 0;
        } else {
            component_len += 1;
            if component_len > NAME_MAX {
                return Err(SyscallError::PathTooLong);
            }
        }
        out.push(byte as char);
    }

    unreachable!()
}

fn execve_strings_from_raw(values: CVec<CString>) -> Result<Vec<String>, SyscallError> {
    Vec::k_from(values).map_err(|err| err.as_syscall_error())
}

fn check_execve_permissions(path: &Path) -> Result<(), SyscallError> {
    let credentials = fs_access_credentials();
    check_access_path_search_permissions(path, &credentials)?;
    let object = open_path(path.clone())?;
    check_access_permissions_for_ids_with_options(&object.stat(), 1, &credentials, false)
}

fn execve_absolute_path(path_str: &str) -> Path {
    let path = Path::new(path_str);
    let fs_context = get_current_process().lock().fs_context.lock().clone();
    AbsolutePath::join_under_root(
        &fs_context.root_directory,
        &fs_context.current_directory,
        &path,
    )
    .as_normal()
}

fn execveat_path(dirfd: i32, path_str: &str, flags: AtFlags) -> Result<Path, SyscallError> {
    const ALLOWED_FLAGS: AtFlags = AtFlags::EMPTY_PATH.union(AtFlags::SYMLINK_NOFOLLOW);

    if flags.bits() != (flags & ALLOWED_FLAGS).bits() {
        return Err(SyscallError::InvalidArguments);
    }

    if path_str.is_empty() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::FileNotFound);
        }
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        let file_like = object.as_file_like()?;
        return Ok(file_like.path());
    }

    let path = Path::new(path_str);
    if path.is_absolute() || dirfd == AT_FDCWD {
        return Ok(execve_absolute_path(path_str));
    }

    let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
    let file_like = object
        .as_file_like()
        .map_err(|_| SyscallError::NotADirectory)?;
    if !matches!(file_like.info()?.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }
    let mut base = AbsolutePath::from_root_path(&file_like.path());
    base.push_path_str(path_str);
    Ok(base.as_normal())
}

fn execve_common(path: Path, args: Vec<String>, env: Vec<String>) -> Result<(), SyscallError> {
    check_execve_permissions(&path)?;
    execve(path, args, env)?;
    log::info!("execve done");
    Ok(())
}

define_syscall!(Execve, |path_str: CString,
                         args: CVec<CString>,
                         env: CVec<CString>| {
    let path_str = execve_path_from_raw(path_str)?;
    let args = execve_strings_from_raw(args)?;
    let env = execve_strings_from_raw(env)?;
    let path = execve_absolute_path(path_str.as_str());
    execve_common(path, args, env)?;
    Ok(0)
});

define_syscall!(Execveat, |dirfd: i32,
                           path_str: CString,
                           args: CVec<CString>,
                           env: CVec<CString>,
                           flags: AtFlags| {
    let path_str = execve_path_from_raw(path_str)?;
    let args = execve_strings_from_raw(args)?;
    let env = execve_strings_from_raw(env)?;
    let path = execveat_path(dirfd, path_str.as_str(), flags)?;
    if flags.contains(AtFlags::SYMLINK_NOFOLLOW)
        && open_path_nofollow(path.clone())?.read_link().is_ok()
    {
        return Err(SyscallError::TooManySymbolicLinks);
    }
    execve_common(path, args, env)?;
    Ok(0)
});

define_syscall!(Exit, |exit_code: u64| {
    exit_current_thread(ProcessExitStatus::from_exit_code(exit_code));
    return_to_scheduler_no_save();
});

define_syscall!(ExitGroup, |exit_code: u64| {
    terminate_process(
        get_current_process(),
        ProcessExitStatus::from_exit_code(exit_code),
    );
    return_to_scheduler_no_save();
});

define_syscall!(Fork, {
    let current = get_current_process();
    let (child_process, _child_thread) = Process::fork(current);
    let pid = child_process.lock().pid.0;
    MANAGER
        .lock()
        .processes
        .insert(child_process.lock().pid, child_process.clone());
    Ok(pid as usize)
});

define_syscall!(Getpid, { Ok(get_current_process().lock().pid.0 as usize) });

define_syscall!(Gettid, { Ok(get_current_thread().lock().id.0 as usize) });

define_syscall!(SetTidAddress, |tidptr: *mut i32| {
    let current = get_current_thread();
    let tid = current.lock().id.0 as i32;
    current.lock().clear_child_tid = tidptr as u64;
    if !tidptr.is_null() {
        user_safe::write(tidptr, &tid)?;
    }
    Ok(tid as usize)
});

define_syscall!(Getpgid, |pid: i32| {
    let pid = if pid == 0 {
        get_current_process().lock().pid.0
    } else {
        pid as u64
    };
    let process = get_process_with_pid(ProcessID(pid))?;
    Ok(process.lock().group_id.0 as usize)
});

define_syscall!(Setpgid, |pid: i32, group_id: i32| {
    let pid = if pid == 0 {
        get_current_process().lock().pid.0
    } else {
        pid as u64
    };
    let process = get_process_with_pid(ProcessID(pid))?;
    let new_group_id = if group_id == 0 { pid } else { group_id as u64 };
    process.lock().group_id.0 = new_group_id;
    Ok(0)
});

define_syscall!(Getsid, |pid: i32| {
    let pid = if pid == 0 {
        get_current_process().lock().pid.0
    } else {
        pid as u64
    };
    let process = get_process_with_pid(ProcessID(pid))?;
    Ok(process.lock().session_id.0 as usize)
});

define_syscall!(Setsid, {
    let current = get_current_process();
    let mut current = current.lock();
    let pid = current.pid.0;
    if current.group_id.0 == pid {
        return Err(SyscallError::PermissionDenied);
    }
    current.group_id.0 = pid;
    current.session_id.0 = pid;
    current.controlling_terminal = None;
    Ok(pid as usize)
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        polling::event::PollableEvent,
        polling::object::Pollable,
        process::{
            FdFlags, Process, ProcessExitStatus,
            group::ProcessGroupID,
            manager::{MANAGER, get_current_process},
            misc::ProcessID,
        },
        signal::Signals,
        systemcall::{
            implementations::{
                EpollCreate1, EpollCtl, EpollWait, Eventfd, Execve, Getpgid, Getpgrp, Getpid,
                Getppid, PidfdOpen, PidfdSendSignal, Poll, Setpgid,
            },
            test::{
                TestLinuxEpollEvent, TestLinuxPollFd, TestLinuxRusage, TestWaitidSigInfo,
                assert_fd_flags, close_test_fd, expect_fd, write_user_cstr,
            },
            test_helpers::{
                SyscallArgs, allocate_user_test_page, assert_linux_layout, expect_errno, expect_ok,
                read_user_value, write_user_value,
            },
            utils::SyscallError,
        },
        thread::THREAD_MANAGER,
    };

    crate::test!(
        process_identity_syscalls,
        "process identity syscalls match current linux task state",
        process_identity_syscalls_match_current_linux_task_state
    );
    crate::test!(
        process_group_syscalls,
        "process group syscalls follow linux pid zero and esrch rules",
        process_group_syscalls_follow_linux_pid_zero_and_esrch_rules
    );
    crate::test!(
        pidfd_and_waitid_syscalls,
        "pidfd_open and waitid follow linux process rules",
        pidfd_and_waitid_syscalls_follow_linux_rules
    );
    crate::test!(
        execve_syscalls,
        "execve syscall semantics follow linux rules",
        execve_syscalls_follow_linux_rules
    );
    crate::test!(
        exit_thread_semantics,
        "exit helper semantics follow linux rules",
        exit_thread_semantics_follow_linux_rules
    );
    crate::test!(
        exit_group_semantics,
        "exit_group helper semantics follow linux rules",
        exit_group_semantics_follow_linux_rules
    );

    fn process_identity_syscalls_match_current_linux_task_state() {
        let (pid, ppid, group_id) = {
            let process = get_current_process();
            let process = process.lock();
            (
                process.pid.0 as usize,
                process
                    .parent
                    .as_ref()
                    .map(|parent| parent.lock().pid.0 as usize)
                    .unwrap_or(0),
                process.group_id.0 as usize,
            )
        };

        expect_ok(SyscallArgs::none().call::<Getpid>(), pid);
        expect_ok(SyscallArgs::none().call::<Getppid>(), ppid);
        expect_ok(SyscallArgs::none().call::<Getpgrp>(), group_id);
    }

    fn process_group_syscalls_follow_linux_pid_zero_and_esrch_rules() {
        let process = get_current_process();
        let old_group = {
            let process = process.lock();
            process.group_id
        };

        expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Setpgid>(), 0);
        {
            let process = process.lock();
            assert_eq!(process.group_id, ProcessGroupID::from_leader(process.pid));
        }
        expect_ok(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getpgid>(),
            get_current_process().lock().group_id.0 as usize,
        );
        expect_errno(
            SyscallArgs::new([u64::from(u32::MAX), 0, 0, 0, 0, 0]).call::<Getpgid>(),
            SyscallError::NoProcess,
        );

        {
            let mut process = process.lock();
            process.group_id = old_group;
        }
    }

    fn pidfd_and_waitid_syscalls_follow_linux_rules() {
        const P_PID: u64 = 1;
        const P_PIDFD: u64 = 3;
        const EPOLL_CTL_ADD: u64 = 1;
        const EPOLLIN: u32 = 0x001;
        const POLLIN: i16 = 0x001;
        const POLLHUP: i16 = 0x010;
        const WNOHANG: u64 = 1;
        const WUNTRACED: u64 = 2;
        const WSTOPPED: u64 = 2;
        const WEXITED: u64 = 4;
        const WBAD: u64 = 0x20;
        const __WCLONE: u64 = 0x8000_0000;
        const WNOWAIT: u64 = 0x0100_0000;
        const CLD_EXITED: i32 = 1;
        const SI_QUEUE: i32 = -1;
        const STOP_STATUS: i32 = 0x7f;

        assert_linux_layout::<TestWaitidSigInfo>(128, 8);
        assert_linux_layout::<TestLinuxRusage>(144, 8);

        let current = get_current_process();

        let child = Process::empty();
        let child_pid = {
            let mut child = child.lock();
            child.pid = ProcessID::new();
            child.parent = Some(current.clone());
            child.group_id = current.lock().group_id;
            child.pid.0
        };
        MANAGER
            .lock()
            .processes
            .insert(ProcessID(child_pid), child.clone());

        let child_pidfd =
            expect_fd(SyscallArgs::new([child_pid, 0, 0, 0, 0, 0]).call::<PidfdOpen>());
        assert_fd_flags(child_pidfd, FdFlags::CLOEXEC);
        assert!(
            get_object_current_process(child_pidfd as u64)
                .expect("pidfd should resolve")
                .as_pidfd()
                .is_ok()
        );
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<PidfdOpen>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([child_pid, 1, 0, 0, 0, 0]).call::<PidfdOpen>(),
            SyscallError::InvalidArguments,
        );
        let info_page = allocate_user_test_page();
        let poll_page = info_page + 512;
        write_user_value(
            poll_page,
            &[TestLinuxPollFd {
                fd: child_pidfd as i32,
                events: POLLIN | POLLHUP,
                revents: -1,
            }],
        );
        expect_ok(
            SyscallArgs::new([poll_page, 1, 0, 0, 0, 0]).call::<Poll>(),
            0,
        );
        assert_eq!(read_user_value::<TestLinuxPollFd>(poll_page).revents, 0);
        let child_pidfd_object = get_object_current_process(child_pidfd as u64)
            .expect("pidfd should resolve")
            .as_pidfd()
            .expect("pidfd fd should point at a pidfd object");
        assert!(!child_pidfd_object.is_event_ready(PollableEvent::CanBeRead));

        child.lock().exit_status = Some(ProcessExitStatus::Exited(7));
        write_user_value(
            poll_page,
            &[TestLinuxPollFd {
                fd: child_pidfd as i32,
                events: POLLIN | POLLHUP,
                revents: 0,
            }],
        );
        expect_ok(
            SyscallArgs::new([poll_page, 1, 0, 0, 0, 0]).call::<Poll>(),
            1,
        );
        let pidfd_poll = read_user_value::<TestLinuxPollFd>(poll_page);
        assert_eq!(pidfd_poll.revents & POLLIN, POLLIN);
        assert_eq!(pidfd_poll.revents & POLLHUP, 0);
        assert!(child_pidfd_object.is_event_ready(PollableEvent::CanBeRead));

        let pidfd_epoll = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<EpollCreate1>());
        let pidfd_event = TestLinuxEpollEvent {
            events: EPOLLIN,
            data: 0x7069_6466,
        };
        write_user_value(info_page + 640, &pidfd_event);
        expect_ok(
            SyscallArgs::new([
                pidfd_epoll as u64,
                EPOLL_CTL_ADD,
                child_pidfd as u64,
                info_page + 640,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([pidfd_epoll as u64, info_page + 704, 1, 0, 0, 0]).call::<EpollWait>(),
            1,
        );
        let pidfd_ready = read_user_value::<TestLinuxEpollEvent>(info_page + 704);
        let pidfd_ready_events = pidfd_ready.events;
        let pidfd_ready_data = pidfd_ready.data;
        assert_eq!(pidfd_ready_events & EPOLLIN, EPOLLIN);
        assert_eq!(pidfd_ready_data, 0x7069_6466);
        close_test_fd(pidfd_epoll);

        expect_ok(
            SyscallArgs::new([
                P_PIDFD,
                child_pidfd as u64,
                info_page,
                WEXITED | WNOWAIT,
                0,
                0,
            ])
            .call::<Waitid>(),
            0,
        );
        let info = read_user_value::<TestWaitidSigInfo>(info_page);
        assert_eq!(info.si_signo, Signal::SIGCHLD as i32);
        assert_eq!(info.si_code, CLD_EXITED);
        assert_eq!(info.si_pid, child_pid as i32);
        assert_eq!(info.si_status, 7);
        assert!(MANAGER.lock().processes.contains_key(&ProcessID(child_pid)));

        let current_pid = get_current_process().lock().pid.0 as i32;
        let current_uid = get_current_process().lock().real_uid;
        let mut queued_siginfo =
            SigInfo::for_process_signal(Signal::SIGUSR1, current_pid, current_uid);
        queued_siginfo.si_code = SI_QUEUE;
        write_user_value(info_page + 128, &queued_siginfo);
        expect_ok(
            SyscallArgs::new([
                child_pidfd as u64,
                Signal::SIGUSR1 as u64,
                info_page + 128,
                0,
                0,
                0,
            ])
            .call::<PidfdSendSignal>(),
            0,
        );
        {
            let child = child.lock();
            assert!(
                child
                    .pending_signals
                    .contains(Signals::from(Signal::SIGUSR1))
            );
            let pending = child.pending_signal_info[Signal::SIGUSR1.index()]
                .expect("siginfo should be stored for pidfd_send_signal");
            assert_eq!(pending.si_signo, Signal::SIGUSR1 as i32);
            assert_eq!(pending.si_code, SI_QUEUE);
            assert_eq!(pending.si_pid, current_pid);
            assert_eq!(pending.si_uid, current_uid);
        }
        expect_ok(
            SyscallArgs::new([child_pidfd as u64, 0, 0, 0, 0, 0]).call::<PidfdSendSignal>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([child_pidfd as u64, 0, info_page + 128, 0, 0, 0])
                .call::<PidfdSendSignal>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                child_pidfd as u64,
                Signal::SIGUSR1 as u64,
                info_page + 128,
                1,
                0,
                0,
            ])
            .call::<PidfdSendSignal>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([P_PID, child_pid, info_page, WEXITED | WNOHANG, 0, 0])
                .call::<Waitid>(),
            0,
        );
        assert!(!MANAGER.lock().processes.contains_key(&ProcessID(child_pid)));

        write_user_value(info_page + 256, &0x55aa55aai32);
        write_user_value(info_page + 320, &[0xa5u8; 144]);

        let wait4_child = Process::empty();
        let wait4_child_pid = {
            let mut child = wait4_child.lock();
            child.pid = ProcessID::new();
            child.parent = Some(current.clone());
            child.group_id = current.lock().group_id;
            child.exit_status = Some(ProcessExitStatus::Exited(9));
            child.pid.0
        };
        MANAGER
            .lock()
            .processes
            .insert(ProcessID(wait4_child_pid), wait4_child.clone());
        expect_ok(
            SyscallArgs::new([
                wait4_child_pid,
                info_page + 256,
                WNOHANG | __WCLONE,
                info_page + 320,
                0,
                0,
            ])
            .call::<Wait4>(),
            wait4_child_pid as usize,
        );
        assert_eq!(read_user_value::<i32>(info_page + 256), 9 << 8);
        assert_eq!(
            read_user_value::<TestLinuxRusage>(info_page + 320).ru_maxrss,
            0
        );
        assert!(
            !MANAGER
                .lock()
                .processes
                .contains_key(&ProcessID(wait4_child_pid))
        );

        let wait4_preserve_child = Process::empty();
        let wait4_preserve_child_pid = {
            let mut child = wait4_preserve_child.lock();
            child.pid = ProcessID::new();
            child.parent = Some(current.clone());
            child.group_id = current.lock().group_id;
            child.exit_status = Some(ProcessExitStatus::Exited(11));
            child.pid.0
        };
        MANAGER.lock().processes.insert(
            ProcessID(wait4_preserve_child_pid),
            wait4_preserve_child.clone(),
        );
        expect_ok(
            SyscallArgs::new([wait4_preserve_child_pid, 0, WNOHANG, 0, 0, 0]).call::<Wait4>(),
            wait4_preserve_child_pid as usize,
        );
        assert!(
            !MANAGER
                .lock()
                .processes
                .contains_key(&ProcessID(wait4_preserve_child_pid))
        );
        get_current_process()
            .lock()
            .pending_signals
            .insert(Signals::from(Signal::SIGCHLD));

        let wait4_sigchld_child = Process::empty();
        let wait4_sigchld_child_pid = {
            let mut child = wait4_sigchld_child.lock();
            child.pid = ProcessID::new();
            child.parent = Some(current.clone());
            child.group_id = current.lock().group_id;
            child.exit_status = Some(ProcessExitStatus::Exited(13));
            child.pid.0
        };
        MANAGER.lock().processes.insert(
            ProcessID(wait4_sigchld_child_pid),
            wait4_sigchld_child.clone(),
        );
        expect_ok(
            SyscallArgs::new([wait4_sigchld_child_pid, 0, WNOHANG, 0, 0, 0]).call::<Wait4>(),
            wait4_sigchld_child_pid as usize,
        );
        assert!(
            !get_current_process()
                .lock()
                .pending_signals
                .contains(Signals::from(Signal::SIGCHLD))
        );

        let stopped_child = Process::empty();
        let stopped_child_pid = {
            let mut child = stopped_child.lock();
            child.pid = ProcessID::new();
            child.parent = Some(current.clone());
            child.group_id = current.lock().group_id;
            child.wait_event = Some(crate::process::wait::ProcessWaitEvent::Stopped {
                status: STOP_STATUS,
                ptrace: false,
            });
            child.threads.push(alloc::sync::Arc::downgrade(
                &crate::thread::thread::Thread::empty(),
            ));
            child.pid.0
        };
        MANAGER
            .lock()
            .processes
            .insert(ProcessID(stopped_child_pid), stopped_child.clone());

        expect_ok(
            SyscallArgs::new([stopped_child_pid, info_page + 256, WNOHANG, 0, 0, 0])
                .call::<Wait4>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(info_page + 256), 9 << 8);
        assert!(stopped_child.lock().wait_event.is_some());

        expect_ok(
            SyscallArgs::new([
                stopped_child_pid,
                info_page + 256,
                WNOHANG | WUNTRACED,
                info_page + 320,
                0,
                0,
            ])
            .call::<Wait4>(),
            stopped_child_pid as usize,
        );
        assert_eq!(read_user_value::<i32>(info_page + 256), STOP_STATUS);
        assert_eq!(
            read_user_value::<TestLinuxRusage>(info_page + 320).ru_nivcsw,
            0
        );
        assert!(stopped_child.lock().wait_event.is_none());

        let stopped_child_wnowait = Process::empty();
        let stopped_child_wnowait_pid = {
            let mut child = stopped_child_wnowait.lock();
            child.pid = ProcessID::new();
            child.parent = Some(current.clone());
            child.group_id = current.lock().group_id;
            child.wait_event = Some(crate::process::wait::ProcessWaitEvent::Stopped {
                status: STOP_STATUS,
                ptrace: false,
            });
            child.threads.push(alloc::sync::Arc::downgrade(
                &crate::thread::thread::Thread::empty(),
            ));
            child.pid.0
        };
        MANAGER.lock().processes.insert(
            ProcessID(stopped_child_wnowait_pid),
            stopped_child_wnowait.clone(),
        );
        expect_ok(
            SyscallArgs::new([
                P_PID,
                stopped_child_wnowait_pid,
                info_page,
                WEXITED | WNOWAIT | WSTOPPED,
                0,
                0,
            ])
            .call::<Waitid>(),
            0,
        );
        assert_eq!(read_user_value::<TestWaitidSigInfo>(info_page).si_code, 5);
        assert!(stopped_child_wnowait.lock().wait_event.is_some());
        expect_ok(
            SyscallArgs::new([
                stopped_child_wnowait_pid,
                info_page + 256,
                WNOHANG | WUNTRACED,
                0,
                0,
                0,
            ])
            .call::<Wait4>(),
            stopped_child_wnowait_pid as usize,
        );
        assert_eq!(read_user_value::<i32>(info_page + 256), STOP_STATUS);
        assert!(stopped_child_wnowait.lock().wait_event.is_none());

        let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        expect_errno(
            SyscallArgs::new([99, 0, 0, WEXITED, 0, 0]).call::<Waitid>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([P_PID, current_pid as u64, 0, WNOHANG, 0, 0]).call::<Waitid>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([P_PIDFD, eventfd as u64, 0, WEXITED, 0, 0]).call::<Waitid>(),
            SyscallError::BadFileDescriptor,
        );
        expect_errno(
            SyscallArgs::new([P_PID, child_pid, 0, WEXITED, 0, 0]).call::<Waitid>(),
            SyscallError::NoChildProcesses,
        );
        expect_errno(
            SyscallArgs::new([current_pid as u64, 0, WBAD, 0, 0, 0]).call::<Wait4>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([i32::MIN as u32 as u64, 0, WNOHANG, 0, 0, 0]).call::<Wait4>(),
            SyscallError::NoProcess,
        );
        expect_errno(
            SyscallArgs::new([(current_pid + 10_000) as u64, 0, WNOHANG, 0, 0, 0]).call::<Wait4>(),
            SyscallError::NoChildProcesses,
        );

        MANAGER.lock().processes.remove(&ProcessID(child_pid));
        MANAGER
            .lock()
            .processes
            .remove(&ProcessID(stopped_child_pid));
        MANAGER
            .lock()
            .processes
            .remove(&ProcessID(stopped_child_wnowait_pid));
        close_test_fd(eventfd);
        close_test_fd(child_pidfd);
    }

    fn execve_syscalls_follow_linux_rules() {
        let page = allocate_user_test_page();

        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Execve>(),
            SyscallError::BadAddress,
        );
        write_user_cstr(page + 512, b"/does-not-exist\0");
        let argv = [page + 512, 0];
        let envp = [0u64];
        write_user_value(page + 640, &argv);
        write_user_value(page + 704, &envp);
        expect_errno(
            SyscallArgs::new([page + 512, page + 640, page + 704, 0, 0, 0]).call::<Execve>(),
            SyscallError::FileNotFound,
        );
    }

    fn exit_thread_semantics_follow_linux_rules() {
        let saved_process_ref = get_current_process();
        let page = allocate_user_test_page();

        write_user_value(page + 448, &99i32);
        let (helper_process, helper_thread) = Process::fork(saved_process_ref.clone());
        let helper_pid = helper_process.lock().pid;
        MANAGER
            .lock()
            .processes
            .insert(helper_pid, helper_process.clone());
        helper_thread.lock().clear_child_tid = page + 448;
        helper_process.lock().exit_status = Some(ProcessExitStatus::Exited(12));
        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
        thread_manager.mark_thread_exited(helper_thread.clone());
        thread_manager.cleanup_exited_threads();
        drop(thread_manager);
        assert_eq!(
            helper_process
                .lock()
                .addrspace
                .read::<i32>((page + 448) as *const i32)
                .expect("child clear_child_tid should be zeroed"),
            0
        );
        MANAGER.lock().processes.remove(&helper_pid);
    }

    fn exit_group_semantics_follow_linux_rules() {
        let exit_group_process = Process::empty();
        exit_group_process.lock().pid = ProcessID::new();
        let terminated_threads = exit_group_process
            .lock()
            .terminate_inner(ProcessExitStatus::from_exit_code(23));
        assert_eq!(
            exit_group_process.lock().exit_status,
            Some(ProcessExitStatus::Exited(23))
        );
        assert!(terminated_threads.is_empty());
    }
}
