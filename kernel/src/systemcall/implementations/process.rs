use alloc::{string::String, sync::Arc, vec::Vec};

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

fn exit_code_to_status(exit_code: u64) -> i32 {
    ((exit_code & 0xff) << 8) as i32
}

define_syscall!(GetProcessParentID, {
    if let Some(parent) = get_current_process().lock().parent.clone() {
        Ok(parent.lock().pid.0 as usize)
    } else {
        Ok(0)
    }
});

define_syscall!(
    WaitForProcessExit,
    |target_process: ProcessID, status_ptr: *mut i32| {
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

                let matches = if target_process.0 == (-1i64) as u64 {
                    is_current_child
                } else if target_process.0 > 0 {
                    *pid == target_process && is_current_child
                } else {
                    false
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
            None => Err(SyscallError::TryAgain),
        }
    }
);

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

define_syscall!(Fork, {
    let mut manager = MANAGER.lock();
    log::debug!("start fork");
    let current = manager.current.clone().unwrap();
    Ok(current.lock().fork(&mut manager).0 as usize)
});

define_syscall!(GetProcessID, {
    Ok(get_current_process().lock().pid.0 as usize)
});

define_syscall!(GetThreadID, {
    Ok(get_current_thread().lock().id.0 as usize)
});

define_syscall!(GetProcessGroupID, |process: ProcessRef| {
    Ok(process.lock().group_id.0 as usize)
});

define_syscall!(SetProcessGroupID, |process: ProcessRef, group_id: u64| {
    process.lock().group_id.0 = group_id;
    Ok(0)
});
