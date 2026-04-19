use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

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

static PID1_TRACE_WINDOW: AtomicU32 = AtomicU32::new(0);
static PID1_TRACE_SEQ: AtomicU64 = AtomicU64::new(1);
const PID1_TRACE_WINDOW_SYSCALLS: u32 = 192;

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
            | SyscallNumber::Faccessat
            | SyscallNumber::Newfstatat
            | SyscallNumber::Statx
            | SyscallNumber::ReadlinkAt
            | SyscallNumber::MkdirAt
            | SyscallNumber::Faccessat2,
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
                | SyscallNumber::Faccessat
                | SyscallNumber::Faccessat2
                | SyscallNumber::Bpf
                | SyscallNumber::Getrandom
                | SyscallNumber::Clone
                | SyscallNumber::Clone3
                | SyscallNumber::Exit
                | SyscallNumber::ExitGroup
        )
    )
}

pub(crate) fn pid1_trace_window_active() -> bool {
    PID1_TRACE_WINDOW.load(Ordering::Relaxed) > 0
}

fn should_trace_pid1_entry(syscall: Option<SyscallNumber>) -> bool {
    pid1_trace_window_active()
        || matches!(
            syscall,
            Some(SyscallNumber::Bpf | SyscallNumber::Uname | SyscallNumber::Faccessat2)
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

fn should_trace_pid23(syscall: Option<SyscallNumber>) -> bool {
    matches!(
        syscall,
        Some(
            SyscallNumber::Execve
                | SyscallNumber::OpenAt
                | SyscallNumber::Newfstatat
                | SyscallNumber::Statx
                | SyscallNumber::ReadlinkAt
                | SyscallNumber::Access
                | SyscallNumber::Mmap
                | SyscallNumber::Mprotect
                | SyscallNumber::Munmap
                | SyscallNumber::Brk
                | SyscallNumber::ArchPrctl
                | SyscallNumber::Getrandom
                | SyscallNumber::Prlimit64
                | SyscallNumber::Exit
                | SyscallNumber::ExitGroup
        )
    )
}

fn should_trace_pid23_block(syscall: Option<SyscallNumber>) -> bool {
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
                | SyscallNumber::Read
                | SyscallNumber::Write
                | SyscallNumber::Writev
        )
    )
}

#[unsafe(no_mangle)]
extern "C" fn syscall_handler(snapshot_ptr: *mut Snapshot) {
    let snapshot = unsafe { &mut *snapshot_ptr };
    let syscall_no = snapshot.rax;
    let syscall = SyscallNumber::from_number(syscall_no as usize);

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

    let current_pid = with_current_process(|proc| proc.pid.0);
    if current_pid == 1
        && (pid1_trace_window_active()
            || matches!(
                syscall,
                Some(SyscallNumber::Bpf | SyscallNumber::Uname | SyscallNumber::Faccessat2)
            ))
    {
        s_println!(
            "pid1 return frame: syscall={:?} result={} rip={:#x} rsp={:#x} rflags={:#x}",
            syscall,
            result,
            snapshot.rip,
            snapshot.rsp,
            snapshot.rflags
        );
    }

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
    let pid1_trace_window = current_pid == 1 && pid1_trace_window_active();
    let pid1_seq = if current_pid == 1 && should_trace_pid1_entry(syscall) {
        let seq = PID1_TRACE_SEQ.fetch_add(1, Ordering::Relaxed);
        let detail = syscall_trace_detail(syscall, arg1, arg2, arg3).unwrap_or_default();
        s_println!(
            "pid1 syscall enter[{}]: {:?}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}){}",
            seq,
            syscall,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            detail
        );
        Some(seq)
    } else {
        None
    };
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
    if (current_pid == 2 || current_pid == 3) && should_trace_pid23_block(syscall) {
        let detail = syscall_trace_detail(syscall, arg1, arg2, arg3).unwrap_or_default();
        s_println!(
            "pid{} blocking syscall enter: {:?}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}){}",
            current_pid,
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

        if current_pid == 1 && syscall == Some(SyscallNumber::Bpf) {
            PID1_TRACE_WINDOW.store(PID1_TRACE_WINDOW_SYSCALLS, Ordering::Relaxed);
        } else if pid1_trace_window {
            PID1_TRACE_WINDOW.fetch_sub(1, Ordering::Relaxed);
        }

        if current_pid == 1
            && (result < 0
                || should_trace_pid1(syscall)
                || should_trace_pid1_block(syscall)
                || pid1_trace_window)
        {
            let detail = syscall_trace_detail(syscall, arg1, arg2, arg3).unwrap_or_default();
            s_println!(
                "pid1 syscall exit[{}]: {:?}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) -> {}{}",
                pid1_seq.unwrap_or(0),
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

        if (current_pid == 2 || current_pid == 3)
            && (result < 0 || should_trace_pid23(syscall) || should_trace_pid23_block(syscall))
        {
            let detail = syscall_trace_detail(syscall, arg1, arg2, arg3).unwrap_or_default();
            s_println!(
                "pid{} syscall exit: {:?}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) -> {}{}",
                current_pid,
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
