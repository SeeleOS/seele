use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    misc::snapshot::Snapshot,
    process::manager::get_current_process,
    process::ptrace::{maybe_stop_current_on_syscall_entry, maybe_stop_current_on_syscall_exit},
    signal::process_current_process_signals,
    systemcall::numbers::SyscallNumber,
    systemcall::table::SYSCALL_TABLE,
    systemcall::utils::SyscallError,
    thread::{
        THREAD_MANAGER, get_current_thread,
        misc::with_current_thread,
        scheduling::{enable_ap_task_scheduling, return_to_scheduler_no_save},
    },
};
use x86_64::registers::model_specific::FsBase;

static FIRST_USER_SYSCALL_LOGGED: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
extern "C" fn syscall_handler(snapshot_ptr: *mut Snapshot) {
    let snapshot = unsafe { &mut *snapshot_ptr };
    let syscall_no = snapshot.rax;

    let thread_ref = get_current_thread();
    let mut thread = thread_ref.lock();
    let fs_base = FsBase::read().as_u64();
    thread.get_appropriate_snapshot().inner = *snapshot;
    thread.get_appropriate_snapshot().fs_base = fs_base;
    thread.last_syscall_no = syscall_no as u64;
    thread.last_user_snapshot = *snapshot;
    thread.last_user_fs_base = fs_base;
    drop(thread);

    maybe_stop_current_on_syscall_entry();

    log_sddm_syscall("enter", syscall_no, None);

    let result = syscall_handler_unwrapped(
        syscall_no,
        snapshot.rdi,
        snapshot.rsi,
        snapshot.rdx,
        snapshot.r10,
        snapshot.r8,
        snapshot.r9,
    );

    snapshot.rax = result;

    log_sddm_syscall("exit", syscall_no, Some(result));

    with_current_thread(|thread| {
        let fs_base = FsBase::read().as_u64();
        thread.get_appropriate_snapshot().inner = *snapshot;
        thread.get_appropriate_snapshot().fs_base = fs_base;
        thread.last_user_snapshot = *snapshot;
        thread.last_user_fs_base = fs_base;
    });

    maybe_stop_current_on_syscall_exit();

    let should_switch = process_current_process_signals(&get_current_process());
    if should_switch {
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .cleanup_exited_threads();
        // Its fine to no_save becuase we've already saved everything manually
        // And returned the value (snapshot.rax = result)
        return_to_scheduler_no_save();
    }
}

fn log_sddm_syscall(phase: &str, syscall_no: isize, result: Option<isize>) {
    crate::process::misc::with_current_process(|process| {
        let command = process
            .command_line
            .first()
            .map(|command| command.as_str())
            .unwrap_or("");
        let parent_command = process
            .parent
            .as_ref()
            .and_then(|parent| {
                parent
                    .lock()
                    .command_line
                    .first()
                    .cloned()
            })
            .unwrap_or_default();
        if command.contains("sddm-diagnostics") {
            return;
        }
        let is_user_manager_chain = command.contains("systemd-executor")
            || parent_command.contains("systemd-executor")
            || (command == "/usr/lib/systemd/systemd" && parent_command.contains("systemd"));
        if !(command.contains("sddm")
            || command.contains("/usr/bin/X")
            || command.contains("Xorg")
            || (command.contains("/bin/sh") && parent_command.contains("sddm"))
            || parent_command.contains("sddm")
            || command.contains("weston")
            || command.contains("kwin")
            || command.contains("plasma")
            || is_user_manager_chain)
        {
            return;
        }

        let syscall = SyscallNumber::from_number(syscall_no as usize);
        if is_user_manager_chain {
            let Some(result) = result else {
                return;
            };
            if result >= 0 && !matches!(syscall_no, 232 | 271 | 441) {
                return;
            }
            crate::s_println!(
                "sddm-syscall exit pid={} cmd={} parent_cmd={} no={} name={:?} result={}",
                process.pid.0,
                command,
                parent_command,
                syscall_no,
                syscall,
                result
            );
            return;
        }

        if !matches!(syscall_no, 56 | 59 | 61 | 62 | 172 | 173 | 247) {
            return;
        }

        match (phase, result) {
            ("enter", None) => crate::s_println!(
                "sddm-syscall enter pid={} cmd={} no={} name={:?}",
                process.pid.0,
                command,
                syscall_no,
                syscall
            ),
            ("exit", Some(result)) => crate::s_println!(
                "sddm-syscall exit pid={} cmd={} parent_cmd={} no={} name={:?} result={}",
                process.pid.0,
                command,
                parent_command,
                syscall_no,
                syscall,
                result
            ),
            _ => {}
        }
    });
}

fn syscall_handler_unwrapped(
    syscall_no: isize,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> isize {
    if !FIRST_USER_SYSCALL_LOGGED.load(Ordering::Acquire) {
        crate::process::misc::with_current_process(|process| {
            if process.pid.0 > 1 && !FIRST_USER_SYSCALL_LOGGED.swap(true, Ordering::AcqRel) {
                enable_ap_task_scheduling();
            }
        });
    }

    if let Some(Some(handler)) = SYSCALL_TABLE.get(syscall_no as usize) {
        match handler(arg1, arg2, arg3, arg4, arg5, arg6) {
            Ok(value) => value as isize,
            Err(err) => err as isize,
        }
    } else {
        SyscallError::NoSyscall as isize
    }
}
