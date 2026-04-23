use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    misc::snapshot::Snapshot,
    process::manager::get_current_process,
    signal::process_current_process_signals,
    systemcall::table::SYSCALL_TABLE,
    systemcall::utils::SyscallError,
    thread::{
        THREAD_MANAGER, get_current_thread, misc::with_current_thread,
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
    thread.get_appropriate_snapshot().inner = *snapshot;
    thread.get_appropriate_snapshot().fs_base = FsBase::read().as_u64();
    drop(thread);

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
            if process.pid.0 > 1
                && !FIRST_USER_SYSCALL_LOGGED.swap(true, Ordering::AcqRel)
            {
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
