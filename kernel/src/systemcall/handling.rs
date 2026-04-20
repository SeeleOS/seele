use crate::{
    misc::snapshot::Snapshot,
    process::misc::with_current_process,
    systemcall::numbers::SyscallNumber,
    systemcall::table::SYSCALL_TABLE,
    systemcall::utils::SyscallError,
    thread::{THREAD_MANAGER, misc::with_current_thread, scheduling::return_to_executor_no_save},
};
use alloc::{collections::BTreeMap, string::String};
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::registers::model_specific::FsBase;

fn should_trace_process(path: &str) -> bool {
    matches!(
        path,
        p if p.ends_with("/systemd-modules-load")
            || p.ends_with("/modprobe")
            || p.ends_with("/kmod")
            || p.ends_with("/systemd-tmpfiles")
            || p.ends_with("/systemd-sysusers")
            || p.ends_with("/systemd-journald")
            || p.ends_with("/systemd-userdbd")
            || p.ends_with("/udevadm")
            || p.ends_with("/systemd-udevd")
    )
}

lazy_static! {
    static ref TRACED_PROCESSES: Mutex<BTreeMap<u64, String>> = Mutex::new(BTreeMap::new());
}

pub fn register_traced_process(pid: u64, path: String) {
    if should_trace_process(&path) {
        TRACED_PROCESSES.lock().insert(pid, path);
    }
}

fn traced_process_path(pid: u64) -> Option<String> {
    TRACED_PROCESSES.lock().get(&pid).cloned()
}

fn should_trace_pid1_syscall(syscall: Option<SyscallNumber>) -> bool {
    matches!(syscall, Some(SyscallNumber::Wait4 | SyscallNumber::Waitid))
}

#[unsafe(no_mangle)]
extern "C" fn syscall_handler(snapshot_ptr: *mut Snapshot) {
    let snapshot = unsafe { &mut *snapshot_ptr };
    let syscall_no = snapshot.rax;

    let thread_ref = THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .current
        .clone()
        .unwrap();
    let mut thread = thread_ref.lock();
    thread.get_appropriate_snapshot().inner = *snapshot;
    thread.get_appropriate_snapshot().fs_base = FsBase::read().as_u64();
    drop(thread);

    let current_pid = with_current_process(|proc| proc.pid.0);
    let syscall = SyscallNumber::try_from(syscall_no as usize).ok();
    if current_pid == 1 && should_trace_pid1_syscall(syscall) {
        crate::s_println!("pid1 trace enter syscall={:?}", syscall);
    }

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
        thread.get_appropriate_snapshot().inner = *snapshot;
        thread.get_appropriate_snapshot().fs_base = FsBase::read().as_u64();
    });

    let should_switch = with_current_process(|proc| proc.process_signals());
    if should_switch {
        let current_pid = with_current_process(|proc| proc.pid.0);
        if current_pid == 32 {
            crate::s_println!("signal cleanup path=syscall pid={}", current_pid);
        }
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .cleanup_exited_threads();
        if current_pid == 32 {
            crate::s_println!("signal cleanup done path=syscall pid={}", current_pid);
        }
        // Its fine to no_save becuase we've already saved everything manually
        // And returned the value (snapshot.rax = result)
        return_to_executor_no_save();
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
    let current_pid = with_current_process(|proc| proc.pid.0);
    let current_name = traced_process_path(current_pid);
    let syscall = SyscallNumber::try_from(syscall_no as usize).ok();
    if let Some(Some(handler)) = SYSCALL_TABLE.get(syscall_no as usize) {
        let result = match handler(arg1, arg2, arg3, arg4, arg5, arg6) {
            Ok(value) => value as isize,
            Err(err) => err as isize,
        };

        if let Some(path) = current_name.as_deref() {
            let should_log = result < 0 || matches!(syscall, Some(SyscallNumber::ExitGroup));
            if should_log {
                crate::s_println!(
                    "trace exit pid={} path={} syscall={:?} result={}",
                    current_pid,
                    path,
                    syscall,
                    result
                );
            }
        } else if current_pid == 1 && should_trace_pid1_syscall(syscall) {
            crate::s_println!("pid1 trace exit syscall={:?} result={}", syscall, result);
        }

        if result < 0 && current_name.is_some() {
            crate::s_println!(
                "syscall error: pid={} no={:?} args=({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) -> {}",
                current_pid,
                syscall,
                arg1,
                arg2,
                arg3,
                arg4,
                arg5,
                arg6,
                result
            );
        }

        if result == SyscallError::BadAddress as isize {
            crate::s_println!(
                "bad address syscall: pid={} no={} args=({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) -> {}",
                current_pid,
                syscall_no,
                arg1,
                arg2,
                arg3,
                arg4,
                arg5,
                arg6,
                result
            );
        }

        result
    } else {
        crate::s_println!(
            "Attempted to call invalid syscall {} pid={} args=({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x})",
            syscall_no,
            current_pid,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6
        );
        SyscallError::NoSyscall as isize
    }
}
