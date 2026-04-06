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
    thread::{THREAD_MANAGER, scheduling::return_to_executor_no_save},
};

define_syscall!(GetProcessParentID, {
    if let Some(parent) = get_current_process().lock().parent.clone() {
        Ok(parent.lock().pid.0 as usize)
    } else {
        Ok(0)
    }
});

define_syscall!(
    WaitForProcessExit,
    |target_process: ProcessID, exit_code_ptr: *mut u64| {
        let current_process = get_current_process();
        let check_result = {
            let manager = MANAGER.lock();
            let process = manager
                .processes
                .iter()
                .find(|(pid, process)| {
                    if target_process.0 == (-1i64) as u64 {
                        // Waiting for any process to exit
                        process
                            .lock()
                            .parent
                            .clone()
                            .is_some_and(|parent| Arc::ptr_eq(&parent, &current_process))
                    } else if target_process.0 > 0 {
                        **pid == target_process
                    } else {
                        false
                    }
                })
                .ok_or(SyscallError::NoProcess)?;

            let p_lock = process.1.lock();
            if p_lock.threads.is_empty() {
                Some((process.1.clone(), p_lock.exit_code.unwrap_or(0)))
            } else {
                None
            }
        };

        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .cleanup_exited_threads();

        match check_result {
            Some((process, exit_code)) => {
                if !exit_code_ptr.is_null() {
                    unsafe {
                        *exit_code_ptr = exit_code;
                    }
                }
                let pid = process.lock().pid.0;
                MANAGER.lock().reap_process(process);
                s_print!("exit");
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
    unreachable!();
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
    Err(SyscallError::other("get tid unimplemented"))
});

define_syscall!(GetProcessGroupID, |process: ProcessRef| {
    Ok(process.lock().group_id.0 as usize)
});

define_syscall!(SetProcessGroupID, |process: ProcessRef, group_id: u64| {
    process.lock().group_id.0 = group_id;
    Ok(0)
});
