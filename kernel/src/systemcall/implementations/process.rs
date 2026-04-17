use alloc::{string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;

use crate::{
    define_syscall,
    process::{
        ProcessRef,
        execve::execve,
        manager::{MANAGER, get_current_process, terminate_process},
        misc::ProcessID,
    },
    s_print,
    systemcall::utils::{SyscallError, SyscallImpl},
    thread::{THREAD_MANAGER, get_current_thread, scheduling::return_to_executor_no_save},
};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct WaitOptions: i32 {
        const NOHANG = 1;
    }
}

fn exit_code_to_status(exit_code: u64) -> i32 {
    ((exit_code & 0xff) << 8) as i32
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
                        options: i32,
                        _rusage: u64| {
    let current_group = get_current_process().lock().group_id;
    let current_process = get_current_process();
    let check_result = {
        let manager = MANAGER.lock();
        let mut matched_child = false;
        let mut exited_child = None;

        for (pid, process) in manager.processes.iter() {
            let p_lock = process.lock();
            let is_current_child = p_lock
                .parent
                .clone()
                .is_some_and(|parent| Arc::ptr_eq(&parent, &current_process));

            let matches = match target_process {
                -1 => is_current_child,
                0 => is_current_child && p_lock.group_id == current_group,
                1.. => pid.0 == target_process as u64 && is_current_child,
                i32::MIN..=-2 => is_current_child && p_lock.group_id.0 == (-target_process) as u64,
            };

            if !matches {
                continue;
            }

            matched_child = true;

            if p_lock.threads.is_empty() {
                exited_child = Some((process.clone(), p_lock.exit_code.unwrap_or(0)));
                break;
            }
        }

        if let Some(process) = exited_child {
            Some(process)
        } else if matched_child {
            None
        } else {
            return Err(SyscallError::NoProcess);
        }
    };

    THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .cleanup_exited_threads();

    match check_result {
        Some((process, exit_code)) => {
            if !status_ptr.is_null() {
                unsafe {
                    *status_ptr = exit_code_to_status(exit_code);
                }
            }
            let pid = process.lock().pid.0;
            MANAGER.lock().reap_process(process);
            Ok(pid as usize)
        }
        None if WaitOptions::from_bits_truncate(options).contains(WaitOptions::NOHANG) => Ok(0),
        None => Err(SyscallError::TryAgain),
    }
});

define_syscall!(Execve, |path_str: String,
                         args: Vec<String>,
                         env: Vec<String>| {
    execve(
        crate::filesystem::path::Path::new(path_str.as_str()),
        args,
        env,
    )?;
    log::info!("execve done");
    Ok(0)
});

define_syscall!(Exit, |exit_code: u64| {
    terminate_process(get_current_process(), exit_code);
    return_to_executor_no_save();
});

define_syscall!(ExitGroup, |exit_code: u64| {
    terminate_process(get_current_process(), exit_code);
    return_to_executor_no_save();
});

define_syscall!(Fork, {
    let current = get_current_process();
    let (child_process, _child_thread) = crate::process::Process::fork(current);
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
    let tid = get_current_thread().lock().id.0 as i32;
    if !tidptr.is_null() {
        unsafe {
            *tidptr = tid;
        }
    }
    Ok(tid as usize)
});

define_syscall!(Getpgid, |pid: i32| {
    let pid = if pid == 0 {
        get_current_process().lock().pid.0
    } else {
        pid as u64
    };
    let process = crate::process::misc::get_process_with_pid(ProcessID(pid))?;
    Ok(process.lock().group_id.0 as usize)
});

define_syscall!(Setpgid, |pid: i32, group_id: i32| {
    let pid = if pid == 0 {
        get_current_process().lock().pid.0
    } else {
        pid as u64
    };
    let process = crate::process::misc::get_process_with_pid(ProcessID(pid))?;
    let new_group_id = if group_id == 0 { pid } else { group_id as u64 };
    process.lock().group_id.0 = new_group_id;
    Ok(0)
});

define_syscall!(Setsid, {
    let current = get_current_process();
    let pid = current.lock().pid.0;
    current.lock().group_id.0 = pid;
    Ok(pid as usize)
});
