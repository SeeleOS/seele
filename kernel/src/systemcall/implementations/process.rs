use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    define_syscall,
    misc::usercopy::write_user_value,
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

fn exit_code_to_status(exit_code: u64) -> u64 {
    (exit_code & 0xff) << 8
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
    |target_process: ProcessID, status_ptr: *mut u64| {
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
                if !status_ptr.is_null()
                    && !write_user_value(status_ptr, exit_code_to_status(exit_code))
                {
                    return Err(SyscallError::BadAddress);
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
    Err(SyscallError::other("get tid unimplemented"))
});

define_syscall!(GetProcessGroupID, |process: ProcessRef| {
    Ok(process.lock().group_id.0 as usize)
});

define_syscall!(SetProcessGroupID, |process: ProcessRef, group_id: u64| {
    process.lock().group_id.0 = group_id;
    Ok(0)
});
