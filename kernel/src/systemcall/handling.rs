use alloc::{format, string::String, vec::Vec};

use crate::{
    memory::user_safe,
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

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxPollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

fn read_user_c_string(ptr: u64, limit: usize) -> Option<String> {
    if ptr == 0 {
        return None;
    }

    let mut bytes = Vec::new();
    for offset in 0..limit {
        let byte = user_safe::read::<u8>((ptr + offset as u64) as *const u8).ok()?;
        if byte == 0 {
            return String::from_utf8(bytes).ok();
        }
        bytes.push(byte);
    }

    Some(String::from("<unterminated>"))
}

fn syscall_trace_detail(
    syscall: Option<SyscallNumber>,
    arg1: u64,
    arg2: u64,
    arg3: u64,
) -> Option<String> {
    match syscall {
        Some(SyscallNumber::Execve) | Some(SyscallNumber::Access) => {
            read_user_c_string(arg1, 160).map(|path| format!(" path={path}"))
        }
        Some(
            SyscallNumber::OpenAt
            | SyscallNumber::Newfstatat
            | SyscallNumber::Statx
            | SyscallNumber::ReadlinkAt
            | SyscallNumber::MkdirAt,
        ) => read_user_c_string(arg2, 160).map(|path| format!(" path={path}")),
        Some(SyscallNumber::Ppoll) => {
            if arg1 == 0 || arg2 == 0 {
                return None;
            }
            let pollfd = user_safe::read::<LinuxPollFd>(arg1 as *const LinuxPollFd).ok()?;
            Some(format!(
                " pollfd[0]={{fd={}, events={:#x}, revents={:#x}}} nfds={}",
                pollfd.fd, pollfd.events, pollfd.revents, arg2
            ))
        }
        Some(SyscallNumber::Poll) => {
            if arg1 == 0 || arg2 == 0 {
                return None;
            }
            let pollfd = user_safe::read::<LinuxPollFd>(arg1 as *const LinuxPollFd).ok()?;
            Some(format!(
                " pollfd[0]={{fd={}, events={:#x}, revents={:#x}}} nfds={}",
                pollfd.fd, pollfd.events, pollfd.revents, arg2
            ))
        }
        _ => None,
    }
}

fn should_trace_pid1(syscall: Option<SyscallNumber>) -> bool {
    matches!(
        syscall,
        Some(
            SyscallNumber::Execve
                | SyscallNumber::OpenAt
                | SyscallNumber::Newfstatat
                | SyscallNumber::Statx
                | SyscallNumber::ReadlinkAt
                | SyscallNumber::Ioctl
                | SyscallNumber::Uname
                | SyscallNumber::Prctl
                | SyscallNumber::Settimeofday
                | SyscallNumber::ClockSettime
                | SyscallNumber::Setrlimit
                | SyscallNumber::Socket
                | SyscallNumber::Connect
                | SyscallNumber::Bind
                | SyscallNumber::Listen
                | SyscallNumber::Setsockopt
                | SyscallNumber::Getrandom
                | SyscallNumber::Clone
                | SyscallNumber::Clone3
        )
    )
}

fn should_trace_pid1_block(syscall: Option<SyscallNumber>) -> bool {
    matches!(
        syscall,
        Some(
            SyscallNumber::Poll
                | SyscallNumber::Ppoll
                | SyscallNumber::Pselect6
                | SyscallNumber::Wait4
                | SyscallNumber::Waitid
                | SyscallNumber::Nanosleep
                | SyscallNumber::ClockNanosleep
                | SyscallNumber::RtSigsuspend
                | SyscallNumber::Futex
        )
    )
}

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
    let current_pid = with_current_process(|proc| proc.pid.0);
    let syscall = SyscallNumber::from_number(syscall_no as usize);
    if current_pid == 1 && should_trace_pid1_block(syscall) {
        let detail = syscall_trace_detail(syscall, arg1, arg2, arg3).unwrap_or_default();
        s_println!(
            "pid1 blocking syscall enter: {:?}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}){}",
            syscall,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            detail
        );
    }

    if let Some(Some(handler)) = SYSCALL_TABLE.get(syscall_no as usize) {
        let result = match handler(arg1, arg2, arg3, arg4, arg5, arg6) {
            Ok(value) => value as isize,
            Err(err) => err as isize,
        };

        if current_pid == 1 && (result < 0 || should_trace_pid1(syscall) || should_trace_pid1_block(syscall))
        {
            let detail = syscall_trace_detail(syscall, arg1, arg2, arg3).unwrap_or_default();
            s_println!(
                "pid1 syscall exit: {:?}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) -> {}{}",
                syscall,
                arg1,
                arg2,
                arg3,
                arg4,
                arg5,
                arg6,
                result,
                detail
            );
        }

        if result == SyscallError::BadAddress as isize {
            s_println!(
                "bad address syscall: pid={} {:?}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) -> {}",
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
        s_println!(
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
