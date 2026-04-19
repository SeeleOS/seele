use crate::{
    misc::snapshot::Snapshot,
    process::misc::with_current_process,
    systemcall::table::SYSCALL_TABLE,
    systemcall::utils::SyscallError,
    thread::{THREAD_MANAGER, misc::with_current_thread, scheduling::return_to_executor_no_save},
};
use x86_64::registers::model_specific::FsBase;

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

    if with_current_process(|proc| proc.process_signals()) {
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
    if let Some(Some(handler)) = SYSCALL_TABLE.get(syscall_no as usize) {
        let result = match handler(arg1, arg2, arg3, arg4, arg5, arg6) {
            Ok(value) => value as isize,
            Err(err) => err as isize,
        };

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
