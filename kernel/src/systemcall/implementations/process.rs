use alloc::{string::String, vec::Vec};

use crate::{
    define_syscall,
    multitasking::{
        MANAGER,
        process::{
            execve::execve,
            manager::get_current_process,
            misc::ProcessID,
        },
        scheduling::return_to_executor_no_save,
        thread::THREAD_MANAGER,
    },
    systemcall::{error::SyscallError, numbers::SyscallNo, utils::SyscallImpl},
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
        let check_result = {
            let manager = MANAGER.lock();
            let process = manager
                .processes
                .iter()
                .find(|(pid, _)| **pid == target_process)
                .ok_or(SyscallError::NoProcess)?;

            let p_lock = process.1.lock();
            if p_lock.threads.is_empty() {
                Some(p_lock.exit_code.unwrap_or(0))
            } else {
                None
            }
        };

        match check_result {
            Some(exit_code) => {
                if !exit_code_ptr.is_null() {
                    unsafe {
                        *exit_code_ptr = exit_code;
                    }
                }
                Ok(0)
            }
            None => Err(SyscallError::TryAgain),
        }
    }
);

define_syscall!(Execve, |path_str: String, args: Vec<String>, env: Vec<String>| {
    execve(crate::filesystem::path::Path::new(path_str.as_str()), args, env)?;
    log::info!("execve done");
    Ok(0)
});

define_syscall!(Exit, |exit_code: u64| {
    let mut manager = THREAD_MANAGER.get().unwrap().lock();
    log::debug!(
        "exit: pid {} code {}",
        get_current_process().lock().pid.0,
        exit_code
    );
    manager.mark_current_as_zombie();
    get_current_process().lock().exit_code = Some(exit_code);
    drop(manager);
    return_to_executor_no_save();
    panic!("What the fuck")
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
