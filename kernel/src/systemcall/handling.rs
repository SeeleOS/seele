use crate::{
    misc::snapshot::Snapshot,
    process::misc::with_current_process,
    s_println,
    systemcall::numbers::SyscallNumber,
    systemcall::table::SYSCALL_TABLE,
    systemcall::utils::SyscallError,
    thread::{
        THREAD_MANAGER,
        misc::with_current_thread,
        scheduling::{return_to_executor_from_current, return_to_executor_no_save},
    },
};
use x86_64::registers::model_specific::FsBase;

#[unsafe(no_mangle)]
extern "C" fn syscall_handler(snapshot_ptr: *mut Snapshot) {
    let snapshot = unsafe { &mut *snapshot_ptr };

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
        snapshot.rax,
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
    let syscall = SyscallNumber::from_number(syscall_no as usize);
    let should_log = with_current_process(|proc| {
        (3..=6).contains(&proc.pid.0)
            && matches!(
                syscall,
                Some(
                    SyscallNumber::ArchPrctl
                        | SyscallNumber::Brk
                        | SyscallNumber::RtSigaction
                        | SyscallNumber::Sigaltstack
                        | SyscallNumber::RtSigprocmask
                        | SyscallNumber::Clone
                        | SyscallNumber::Clone3
                        | SyscallNumber::SetTidAddress
                        | SyscallNumber::Futex
                        | SyscallNumber::Rseq
                        | SyscallNumber::Prctl
                        | SyscallNumber::Getrandom
                        | SyscallNumber::ClockGettime
                        | SyscallNumber::Gettimeofday
                        | SyscallNumber::Pipe
                        | SyscallNumber::Pipe2
                        | SyscallNumber::Socket
                        | SyscallNumber::Socketpair
                        | SyscallNumber::Connect
                        | SyscallNumber::Bind
                        | SyscallNumber::Listen
                        | SyscallNumber::Accept
                        | SyscallNumber::Getsockname
                        | SyscallNumber::Getpeername
                        | SyscallNumber::Setsockopt
                        | SyscallNumber::Getsockopt
                        | SyscallNumber::OpenAt
                        | SyscallNumber::Close
                        | SyscallNumber::Read
                        | SyscallNumber::Write
                        | SyscallNumber::Readlink
                        | SyscallNumber::ReadlinkAt
                        | SyscallNumber::Getcwd
                        | SyscallNumber::Fstat
                        | SyscallNumber::Newfstatat
                        | SyscallNumber::Statx
                        | SyscallNumber::Statfs
                        | SyscallNumber::Getdents
                        | SyscallNumber::Getdents64
                        | SyscallNumber::Prlimit64
                        | SyscallNumber::Mmap
                        | SyscallNumber::Munmap
                        | SyscallNumber::Mremap
                        | SyscallNumber::Mprotect
                        | SyscallNumber::Madvise
                        | SyscallNumber::Ioctl
                )
            )
    });

    if should_log {
        match syscall {
            Some(number) => s_println!(
                "syscall enter: pid={} {:?}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x})",
                with_current_process(|proc| proc.pid.0),
                number,
                arg1,
                arg2,
                arg3,
                arg4,
                arg5,
                arg6
            ),
            None => s_println!(
                "syscall enter: {}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x})",
                syscall_no,
                arg1,
                arg2,
                arg3,
                arg4,
                arg5,
                arg6
            ),
        }
    }

    if let Some(Some(handler)) = SYSCALL_TABLE.get(syscall_no as usize) {
        let result = match handler(arg1, arg2, arg3, arg4, arg5, arg6) {
            Ok(value) => value as isize,
            Err(err) => err as isize,
        };

        if should_log {
            match syscall {
                Some(number) => s_println!(
                    "syscall exit: pid={} {:?} -> {}",
                    with_current_process(|proc| proc.pid.0),
                    number,
                    result
                ),
                None => s_println!("syscall exit: {} -> {}", syscall_no, result),
            }
        }

        result
    } else {
        s_println!("Attempted to call invalid syscall {}", syscall_no);
        SyscallError::NoSyscall as isize
    }
}
