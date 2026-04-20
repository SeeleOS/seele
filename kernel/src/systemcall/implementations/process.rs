use alloc::{string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;

use crate::{
    define_syscall,
    filesystem::path::Path,
    memory::user_safe,
    misc::signal::SigInfo,
    object::misc::get_object_current_process,
    process::{
        Process, ProcessRef,
        execve::execve,
        manager::{MANAGER, get_current_process, terminate_process},
        misc::{ProcessID, get_process_with_pid},
    },
    signal::Signal,
    systemcall::utils::{SyscallError, SyscallImpl},
    thread::{
        THREAD_MANAGER, get_current_thread,
        scheduling::return_to_executor_no_save,
        yielding::{BlockType, WakeType, finish_block_current, prepare_block_current},
    },
};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct WaitOptions: i32 {
        const NOHANG = 1;
        const WEXITED = 4;
        const WNOWAIT = 0x0100_0000;
    }
}

const CLD_EXITED: i32 = 1;

fn exit_code_to_status(exit_code: u64) -> i32 {
    ((exit_code & 0xff) << 8) as i32
}

fn has_wait_interrupt_signal(process: &ProcessRef) -> bool {
    let mut pending = process.lock().pending_signals;
    pending.remove(Signal::ChildChanged.into());
    !pending.is_empty()
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
    let wait_options = WaitOptions::from_bits_truncate(options);
    let preserve_child = wait_options.contains(WaitOptions::WNOWAIT);
    let current_process = get_current_process();
    let current_pid = current_process.lock().pid.0;
    if current_pid == 1 {
        crate::s_println!(
            "pid1 wait4 enter target={} options={:#x} preserve_child={}",
            target_process,
            options,
            preserve_child
        );
    }
    loop {
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .cleanup_exited_threads();

        let current_group = current_process.lock().group_id;
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
                    i32::MIN..=-2 => {
                        is_current_child && p_lock.group_id.0 == (-target_process) as u64
                    }
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
                return Err(SyscallError::NoChildProcesses);
            }
        };

        match check_result {
            Some((process, exit_code)) => {
                if !status_ptr.is_null() {
                    let status = exit_code_to_status(exit_code);
                    user_safe::write(status_ptr, &status)?;
                }
                let pid = process.lock().pid.0;
                if current_pid == 1 {
                    crate::s_println!(
                        "pid1 wait4 return target={} child_pid={} exit_code={} preserve_child={}",
                        target_process,
                        pid,
                        exit_code,
                        preserve_child
                    );
                }
                if current_pid == 26 || pid == 32 {
                    crate::s_println!(
                        "wait4 return current_pid={} target={} child_pid={} exit_code={}",
                        current_pid,
                        target_process,
                        pid,
                        exit_code
                    );
                }
                if !preserve_child {
                    MANAGER.lock().reap_process(process);
                }
                return Ok(pid as usize);
            }
            None if wait_options.contains(WaitOptions::NOHANG) => {
                if current_pid == 1 {
                    crate::s_println!(
                        "pid1 wait4 nohang-empty target={} options={:#x}",
                        target_process,
                        options
                    );
                }
                return Ok(0);
            }
            None => {
                if has_wait_interrupt_signal(&current_process) {
                    if current_pid == 1 {
                        crate::s_println!("pid1 wait4 interrupted-before-block target={}", target_process);
                    }
                    return Err(SyscallError::Interrupted);
                }

                if current_pid == 1 {
                    crate::s_println!("pid1 wait4 block target={} options={:#x}", target_process, options);
                }
                if current_pid == 26 || target_process == 32 {
                    crate::s_println!(
                        "wait4 block current_pid={} target={}",
                        current_pid,
                        target_process
                    );
                }
                prepare_block_current(BlockType::WakeRequired {
                    wake_type: WakeType::ProcsesExit,
                    deadline: None,
                });
                finish_block_current();

                if current_pid == 26 || target_process == 32 {
                    crate::s_println!(
                        "wait4 wake current_pid={} target={}",
                        current_pid,
                        target_process
                    );
                }
                if current_pid == 1 {
                    crate::s_println!("pid1 wait4 wake target={} options={:#x}", target_process, options);
                }

                if has_wait_interrupt_signal(&current_process) {
                    if current_pid == 1 {
                        crate::s_println!("pid1 wait4 interrupted-after-wake target={}", target_process);
                    }
                    return Err(SyscallError::Interrupted);
                }
            }
        }
    }
});

define_syscall!(Waitid, |id_type: i32,
                         id: u32,
                         info_ptr: *mut SigInfo,
                         options: i32| {
    if get_current_process().lock().pid.0 == 1 {
        crate::s_println!(
            "pid1 waitid enter id_type={} id={} options={:#x}",
            id_type,
            id,
            options
        );
    }
    let target_process = match id_type {
        0 => -1,
        1 => id as i32,
        2 => -(id as i32),
        3 => get_object_current_process(id as u64)?.as_pidfd()?.pid() as i32,
        _ => return Err(SyscallError::InvalidArguments),
    };

    if options & WaitOptions::WEXITED.bits() == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let mut status = 0;
    let pid = Wait4::handle_call(
        target_process as u64,
        (&mut status as *mut i32) as u64,
        options as u64,
        0,
        0,
        0,
    )?;

    if !info_ptr.is_null() {
        let info = if pid == 0 {
            SigInfo::default()
        } else {
            SigInfo::for_waitid(
                Signal::ChildChanged,
                CLD_EXITED,
                pid as i32,
                (status >> 8) & 0xff,
            )
        };
        user_safe::write(info_ptr, &info)?;
    }

    if get_current_process().lock().pid.0 == 1 {
        crate::s_println!("pid1 waitid return id_type={} id={} pid={}", id_type, id, pid);
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
    terminate_process(get_current_process(), exit_code);
    return_to_executor_no_save();
});

define_syscall!(ExitGroup, |exit_code: u64| {
    terminate_process(get_current_process(), exit_code);
    return_to_executor_no_save();
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

define_syscall!(Setsid, {
    let current = get_current_process();
    let pid = current.lock().pid.0;
    current.lock().group_id.0 = pid;
    Ok(pid as usize)
});
