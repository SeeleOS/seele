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
    let syscall_debug = syscall_debug_target(syscall_no as usize);

    let thread_ref = get_current_thread();
    let mut thread = thread_ref.lock();
    let fs_base = FsBase::read().as_u64();
    thread.get_appropriate_snapshot().inner = *snapshot;
    thread.get_appropriate_snapshot().fs_base = fs_base;
    thread.last_syscall_no = syscall_no as u64;
    thread.last_user_snapshot = *snapshot;
    thread.last_user_fs_base = fs_base;
    drop(thread);

    if let Some(syscall_name) = syscall_debug {
        log_syscall_event(
            "enter",
            syscall_name,
            syscall_no,
            snapshot.rdi,
            snapshot.rsi,
            snapshot.rdx,
            snapshot.r10,
            snapshot.r8,
            snapshot.r9,
        );
    }

    maybe_stop_current_on_syscall_entry();

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

    with_current_thread(|thread| {
        let fs_base = FsBase::read().as_u64();
        thread.get_appropriate_snapshot().inner = *snapshot;
        thread.get_appropriate_snapshot().fs_base = fs_base;
        thread.last_user_snapshot = *snapshot;
        thread.last_user_fs_base = fs_base;
    });

    if let Some(syscall_name) = syscall_debug {
        log_syscall_exit(syscall_name, syscall_no, result);
    }

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

fn syscall_debug_target(syscall_no: usize) -> Option<&'static str> {
    if !should_log_x_syscall() {
        return None;
    }

    match SyscallNumber::from_number(syscall_no)? {
        SyscallNumber::OpenAt => Some("openat"),
        SyscallNumber::Mmap => Some("mmap"),
        SyscallNumber::Ioctl => Some("ioctl"),
        SyscallNumber::Futex => Some("futex"),
        SyscallNumber::Poll => Some("poll"),
        SyscallNumber::Ppoll => Some("ppoll"),
        SyscallNumber::EpollWait => Some("epoll_wait"),
        SyscallNumber::EpollCtl => Some("epoll_ctl"),
        SyscallNumber::EpollPwait => Some("epoll_pwait"),
        SyscallNumber::EpollCreate1 => Some("epoll_create1"),
        SyscallNumber::EpollPwait2 => Some("epoll_pwait2"),
        _ => None,
    }
}

fn should_log_x_syscall() -> bool {
    crate::process::misc::with_current_process(|process| {
        let Some(command) = process.command_line.first() else {
            return false;
        };
        let Some(name) = command.rsplit('/').next() else {
            return false;
        };
        matches!(name, "X" | "Xorg")
    })
}

fn log_syscall_event(
    phase: &str,
    syscall_name: &str,
    syscall_no: isize,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) {
    crate::process::misc::with_current_process(|process| {
        crate::s_println!(
            "xsys {} pid={} {}({}) a1={:#x} a2={:#x} a3={:#x} a4={:#x} a5={:#x} a6={:#x}",
            phase,
            process.pid.0,
            syscall_name,
            syscall_no,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6
        );
    });
}

fn log_syscall_exit(syscall_name: &str, syscall_no: isize, result: isize) {
    crate::process::misc::with_current_process(|process| {
        crate::s_println!(
            "xsys exit pid={} {}({}) ret={:#x}",
            process.pid.0,
            syscall_name,
            syscall_no,
            result
        );
    });
}
