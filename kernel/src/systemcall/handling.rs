use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    misc::snapshot::Snapshot,
    process::manager::get_current_process,
    process::ptrace::{maybe_stop_current_on_syscall_entry, maybe_stop_current_on_syscall_exit},
    signal::process_current_process_signals,
    systemcall::numbers::SyscallNumber,
    systemcall::table::SYSCALL_TABLE,
    systemcall::utils::{SyscallError, log_unsupported_syscall_result},
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
            [
                snapshot.rdi,
                snapshot.rsi,
                snapshot.rdx,
                snapshot.r10,
                snapshot.r8,
                snapshot.r9,
            ],
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
    if result < 0 {
        log_unsupported_syscall_result(
            syscall_no,
            [
                snapshot.rdi,
                snapshot.rsi,
                snapshot.rdx,
                snapshot.r10,
                snapshot.r8,
                snapshot.r9,
            ],
            SyscallError::from(result),
        );
    }

    if syscall_debug.is_some() {
        log_syscall_stage("handler-return", syscall_no, result);
    }

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

    if syscall_debug.is_some() {
        log_syscall_stage("after-exit-stop", syscall_no, result);
    }

    let should_switch = process_current_process_signals(&get_current_process());
    if syscall_debug.is_some() {
        log_syscall_stage(
            if should_switch {
                "after-signal-switch"
            } else {
                "after-signal-noswitch"
            },
            syscall_no,
            result,
        );
    }
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
    if !should_log_boot_debug_syscall() {
        return None;
    }

    match SyscallNumber::from_number(syscall_no)? {
        SyscallNumber::OpenAt => Some("openat"),
        SyscallNumber::Fstat => Some("fstat"),
        SyscallNumber::Newfstatat => Some("newfstatat"),
        SyscallNumber::Read => Some("read"),
        SyscallNumber::Write => Some("write"),
        SyscallNumber::Ioctl => Some("ioctl"),
        SyscallNumber::Close => Some("close"),
        SyscallNumber::Getdents64 => Some("getdents64"),
        SyscallNumber::Fcntl => Some("fcntl"),
        SyscallNumber::Fsync => Some("fsync"),
        SyscallNumber::Ftruncate => Some("ftruncate"),
        SyscallNumber::Fchmod => Some("fchmod"),
        SyscallNumber::Fchmodat => Some("fchmodat"),
        SyscallNumber::Fchmodat2 => Some("fchmodat2"),
        SyscallNumber::Fchown => Some("fchown"),
        SyscallNumber::Fchownat => Some("fchownat"),
        SyscallNumber::Lseek => Some("lseek"),
        SyscallNumber::Rename => Some("rename"),
        SyscallNumber::RenameAt => Some("renameat"),
        SyscallNumber::RenameAt2 => Some("renameat2"),
        SyscallNumber::Unlink => Some("unlink"),
        SyscallNumber::UnlinkAt => Some("unlinkat"),
        SyscallNumber::Link => Some("link"),
        SyscallNumber::LinkAt => Some("linkat"),
        SyscallNumber::Mkdir => Some("mkdir"),
        SyscallNumber::MkdirAt => Some("mkdirat"),
        SyscallNumber::Futex => Some("futex"),
        SyscallNumber::Poll => Some("poll"),
        SyscallNumber::Ppoll => Some("ppoll"),
        SyscallNumber::Getrandom => Some("getrandom"),
        SyscallNumber::Setxattr => Some("setxattr"),
        SyscallNumber::Getxattr => Some("getxattr"),
        SyscallNumber::Fgetxattr => Some("fgetxattr"),
        SyscallNumber::Fsetxattr => Some("fsetxattr"),
        SyscallNumber::Removexattr => Some("removexattr"),
        SyscallNumber::Fremovexattr => Some("fremovexattr"),
        _ => None,
    }
}

fn should_log_boot_debug_syscall() -> bool {
    crate::process::misc::with_current_process(|process| {
        let Some(command) = process.command_line.first() else {
            return false;
        };
        let Some(name) = command.rsplit('/').next() else {
            return false;
        };
        matches!(
            name,
            "init"
                | "systemd"
                | "systemd-sysusers"
                | "systemd-random-seed"
                | "agetty"
                | "login"
        )
    })
}

fn log_syscall_event(phase: &str, syscall_name: &str, syscall_no: isize, args: [u64; 6]) {
    crate::process::misc::with_current_process(|process| {
        let tid = crate::thread::get_current_thread().lock().id.0;
        let name = process
            .command_line
            .first()
            .and_then(|command| command.rsplit('/').next())
            .unwrap_or("?");
        crate::s_println!(
            "bootsys {} comm={} pid={} tid={} {}({}) a1={:#x} a2={:#x} a3={:#x} a4={:#x} a5={:#x} a6={:#x}",
            phase,
            name,
            process.pid.0,
            tid,
            syscall_name,
            syscall_no,
            args[0],
            args[1],
            args[2],
            args[3],
            args[4],
            args[5]
        );
    });
}

fn log_syscall_exit(syscall_name: &str, syscall_no: isize, result: isize) {
    crate::process::misc::with_current_process(|process| {
        let tid = crate::thread::get_current_thread().lock().id.0;
        let name = process
            .command_line
            .first()
            .and_then(|command| command.rsplit('/').next())
            .unwrap_or("?");
        crate::s_println!(
            "bootsys exit comm={} pid={} tid={} {}({}) ret={:#x}",
            name,
            process.pid.0,
            tid,
            syscall_name,
            syscall_no,
            result
        );
    });
}

fn log_syscall_stage(stage: &str, syscall_no: isize, result: isize) {
    crate::process::misc::with_current_process(|process| {
        let tid = crate::thread::get_current_thread().lock().id.0;
        let name = process
            .command_line
            .first()
            .and_then(|command| command.rsplit('/').next())
            .unwrap_or("?");
        crate::s_println!(
            "bootsys stage={} comm={} pid={} tid={} nr={} ret={:#x}",
            stage,
            name,
            process.pid.0,
            tid,
            syscall_no,
            result
        );
    });
}
