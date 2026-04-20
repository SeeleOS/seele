use crate::{
    misc::snapshot::Snapshot,
    process::misc::with_current_process,
    systemcall::numbers::SyscallNumber,
    systemcall::table::SYSCALL_TABLE,
    systemcall::utils::SyscallError,
    thread::{THREAD_MANAGER, misc::with_current_thread, scheduling::return_to_executor_no_save},
};
use alloc::string::String;
use x86_64::registers::model_specific::FsBase;

pub fn register_traced_process(_pid: u64, _path: String) {}

fn current_process_matches_suffix(suffix: &str) -> bool {
    with_current_process(|process| {
        process
            .command_line
            .first()
            .is_some_and(|path| path.ends_with(suffix))
    })
}

fn current_process_is_journald() -> bool {
    current_process_matches_suffix("/systemd-journald")
}

fn current_process_is_udevd() -> bool {
    current_process_matches_suffix("/systemd-udevd")
}

#[unsafe(no_mangle)]
extern "C" fn syscall_handler(snapshot_ptr: *mut Snapshot) {
    let snapshot = unsafe { &mut *snapshot_ptr };
    let syscall_no = snapshot.rax;
    let syscall = SyscallNumber::try_from(syscall_no as usize).ok();

    if current_process_is_journald()
        && matches!(
            syscall,
            Some(SyscallNumber::Exit | SyscallNumber::ExitGroup)
        )
    {
        crate::s_println!(
            "journald exit trace pid={} syscall={:?} code={}",
            with_current_process(|proc| proc.pid.0),
            syscall,
            snapshot.rdi
        );
    }

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

    let should_switch = with_current_process(|proc| proc.process_signals());
    if should_switch {
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .cleanup_exited_threads();
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
    let current_is_journald = current_process_is_journald();
    let current_is_udevd = current_process_is_udevd();
    let syscall = SyscallNumber::try_from(syscall_no as usize).ok();

    if let Some(Some(handler)) = SYSCALL_TABLE.get(syscall_no as usize) {
        let result = match handler(arg1, arg2, arg3, arg4, arg5, arg6) {
            Ok(value) => value as isize,
            Err(err) => err as isize,
        };

        if current_is_journald && result < 0 {
            crate::s_println!(
                "journald syscall error: pid={} no={:?} args=({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) -> {}",
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

        if current_is_udevd
            && matches!(
                syscall,
                Some(
                    SyscallNumber::Exit
                        | SyscallNumber::ExitGroup
                        | SyscallNumber::Sendmsg
                        | SyscallNumber::Recvmsg
                )
            )
        {
            crate::s_println!(
                "udevd syscall trace: pid={} no={:?} args=({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) -> {}",
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

        if current_is_udevd && result < 0 {
            crate::s_println!(
                "udevd syscall error: pid={} no={:?} args=({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) -> {}",
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
