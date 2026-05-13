use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    misc::{
        profile::{self, ProfileCategory},
        snapshot::Snapshot,
        time::Time,
    },
    process::manager::get_current_process,
    process::ptrace::{maybe_stop_current_on_syscall_entry, maybe_stop_current_on_syscall_exit},
    signal::process_current_process_signals,
    systemcall::table::SYSCALL_TABLE,
    systemcall::utils::{
        SyscallError, log_syscall_trace_enter, log_syscall_trace_exit,
        log_unsupported_syscall_result,
    },
    thread::{
        get_current_thread,
        misc::{BlockedSyscall, State, with_current_thread},
        scheduling::{
            enable_ap_task_scheduling, note_user_mode_resume, return_to_scheduler_no_save,
        },
    },
};
use x86_64::registers::model_specific::FsBase;

static FIRST_USER_SYSCALL_LOGGED: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
extern "C" fn syscall_handler(snapshot_ptr: *mut Snapshot) {
    let snapshot = unsafe { &mut *snapshot_ptr };
    let syscall_no = snapshot.rax;
    let syscall_start = profile::scope_start();
    let entry_start = syscall_start;

    let thread_ref = get_current_thread();
    let mut thread = thread_ref.lock();
    let fs_base = FsBase::read().as_u64();
    let thread_snapshot = thread.get_appropriate_snapshot();
    thread_snapshot.inner = *snapshot;
    thread_snapshot.fs_base = fs_base;
    thread_snapshot.snapshot_type = crate::thread::snapshot::ThreadSnapshotType::Thread;
    thread.last_syscall_no = syscall_no as u64;
    thread.last_user_snapshot = *snapshot;
    thread.last_user_fs_base = fs_base;
    thread.active_syscall_profile = BlockedSyscall::from_syscall_number(syscall_no as usize);
    thread.blocked_syscall_started_at = None;
    thread.blocked_syscall_cycles = 0;
    drop(thread);
    let entry_cycles = profile::scope_start().saturating_sub(entry_start);

    let body_start = profile::scope_start();
    maybe_stop_current_on_syscall_entry();

    let args = [
        snapshot.rdi,
        snapshot.rsi,
        snapshot.rdx,
        snapshot.r10,
        snapshot.r8,
        snapshot.r9,
    ];
    log_syscall_trace_enter(syscall_no, args);

    let result = syscall_handler_unwrapped(
        syscall_no, args[0], args[1], args[2], args[3], args[4], args[5],
    );
    if result < 0 {
        log_unsupported_syscall_result(syscall_no, args, SyscallError::from(result));
    }
    log_syscall_trace_exit(syscall_no, args, result);

    snapshot.rax = result;
    let body_end = profile::scope_start();

    let exit_start = profile::scope_start();
    let blocked_cycles = {
        let mut thread = thread_ref.lock();
        if thread.blocked_syscall_started_at.is_some() {
            profile::finish_blocked_syscall(&mut thread);
        }
        thread.blocked_syscall_cycles
    };
    let elapsed = profile::scope_start().saturating_sub(syscall_start);
    let cpu_cycles = elapsed.saturating_sub(blocked_cycles);
    profile::record_cycles(ProfileCategory::SyscallCpu, cpu_cycles);
    profile::record_syscall_cpu(syscall_no as usize, cpu_cycles);
    profile::record_cycles(ProfileCategory::SyscallEntry, entry_cycles);
    profile::record_cycles(
        ProfileCategory::SyscallBody,
        body_end
            .saturating_sub(body_start)
            .saturating_sub(blocked_cycles),
    );

    with_current_thread(|thread| {
        let fs_base = FsBase::read().as_u64();
        let thread_snapshot = thread.get_appropriate_snapshot();
        thread_snapshot.inner = *snapshot;
        thread_snapshot.fs_base = fs_base;
        thread_snapshot.snapshot_type = crate::thread::snapshot::ThreadSnapshotType::Thread;
        thread.last_user_snapshot = *snapshot;
        thread.last_user_fs_base = fs_base;
        thread.active_syscall_profile = None;
        thread.blocked_syscall_started_at = None;
        thread.blocked_syscall_cycles = 0;
    });

    maybe_stop_current_on_syscall_exit();

    if matches!(get_current_thread().lock().state, State::Exiting) {
        profile::record(ProfileCategory::SyscallExit, exit_start);
        return_to_scheduler_no_save();
    }

    let should_switch = process_current_process_signals(&get_current_process());
    if should_switch {
        crate::thread::with_thread_manager(|manager| manager.cleanup_exited_threads());
        // Its fine to no_save becuase we've already saved everything manually
        // And returned the value (snapshot.rax = result)
        profile::record(ProfileCategory::SyscallExit, exit_start);
        return_to_scheduler_no_save();
    }

    note_user_mode_resume(Time::since_boot().as_nanoseconds());
    profile::record(ProfileCategory::SyscallExit, exit_start);
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
