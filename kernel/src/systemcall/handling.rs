use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    misc::snapshot::Snapshot,
    process::manager::get_current_process,
    process::misc::with_current_process,
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
    let Some((pid, command)) = with_current_process(|process| {
        let pid = process.pid.0;
        let command = process.command_line.first().cloned().unwrap_or_default();
        ((command == "systemd" && pid != 1)
            || command.contains("Xorg")
            || command.contains("/usr/bin/X")
            || command == "sleep"
            || command.ends_with("/sleep")
            || command.contains("startplasma")
            || command.contains("kwin")
            || command.contains("plasmashell")
            || command.contains("dbus-broker")
            || command.contains("systemd-user-runtime-dir")
            || (command.ends_with("/systemd") && pid != 1))
        .then_some((pid, command))
    }) else {
        return;
    };

    let syscall_name = SyscallNumber::from_number(syscall_no as usize);
    let should_log = matches!(
        syscall_name,
        Some(
            SyscallNumber::Poll
                | SyscallNumber::Read
                | SyscallNumber::Write
                | SyscallNumber::Writev
                | SyscallNumber::Connect
                | SyscallNumber::Sendto
                | SyscallNumber::Recvfrom
                | SyscallNumber::Sendmsg
                | SyscallNumber::Recvmsg
                | SyscallNumber::Setsockopt
                | SyscallNumber::Getsockopt
                | SyscallNumber::EpollWait
                | SyscallNumber::Ppoll
                | SyscallNumber::EpollPwait
                | SyscallNumber::EpollPwait2
        )
    );
    if !should_log {
        return;
    }

    match (phase, syscall_name, result) {
        ("enter", Some(name), _) => crate::s_println!(
            "display-syscall-enter pid={} cmd={} syscall={:?}({})",
            pid,
            command,
            name,
            syscall_no
        ),
        ("enter", None, _) => crate::s_println!(
            "display-syscall-enter pid={} cmd={} syscall={}",
            pid,
            command,
            syscall_no
        ),
        ("exit", Some(name), Some(value)) => crate::s_println!(
            "display-syscall-exit pid={} cmd={} syscall={:?}({}) result={}",
            pid,
            command,
            name,
            syscall_no,
            value
        ),
        ("exit", None, Some(value)) => crate::s_println!(
            "display-syscall-exit pid={} cmd={} syscall={} result={}",
            pid,
            command,
            syscall_no,
            value
        ),
        _ => {}
    }
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
