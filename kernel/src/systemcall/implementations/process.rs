use alloc::{string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;

use crate::{
    define_syscall,
    filesystem::path::Path,
    memory::user_safe,
    misc::signal::SigInfo,
    object::misc::get_object_current_process,
    process::{
        Process, ProcessExitStatus, ProcessRef,
        execve::execve,
        manager::{MANAGER, exit_current_thread, get_current_process, terminate_process},
        misc::{ProcessID, get_process_with_pid},
        wait::{ProcessWaitEvent, take_wait_event},
    },
    signal::Signal,
    systemcall::utils::{SyscallError, SyscallImpl},
    thread::{
        THREAD_MANAGER, get_current_thread,
        scheduling::return_to_scheduler_no_save,
        yielding::{
            BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        },
    },
};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Wait4Options: i32 {
        const NOHANG = 1;
        const WUNTRACED = 2;
        const WCONTINUED = 8;
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
    let mut pending = process.lock().pending_signals;
    pending.remove(Signal::SIGCHLD.into());
    !pending.is_empty()
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

        if wait_behavior.report_exited && p_lock.threads.is_empty() {
            ready_child = Some(WaitOutcome::Exited(
                process.clone(),
                pid.0,
                p_lock.exit_status.unwrap_or(ProcessExitStatus::Exited(0)),
            ));
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
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .cleanup_exited_threads();

        let check_result = check_wait_outcome(target_process, wait_behavior, &current_process)?;

        match check_result {
            Some(WaitOutcome::Exited(process, pid, exit_status)) => {
                if !wait_behavior.preserve_child {
                    MANAGER.lock().reap_process(process.clone());
                }
                return Ok(Some(WaitOutcome::Exited(process, pid, exit_status)));
            }
            Some(WaitOutcome::Stopped(process, pid, wait_event)) => {
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
        let info = if let Some(result) = result {
            match result {
                WaitOutcome::Exited(_, pid, exit_status) => SigInfo::for_waitid(
                    Signal::SIGCHLD,
                    exit_status.waitid_code(),
                    pid as i32,
                    exit_status.waitid_status(),
                ),
                WaitOutcome::Stopped(_, pid, wait_event) => SigInfo::for_waitid(
                    Signal::SIGCHLD,
                    if wait_event.is_ptrace() {
                        CLD_TRAPPED
                    } else {
                        CLD_STOPPED
                    },
                    pid as i32,
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

define_syscall!(Execve, |path_str: String,
                         args: Vec<String>,
                         env: Vec<String>| {
    let path = Path::new(path_str.as_str());
    execve(path, args, env)?;
    log::info!("execve done");
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
