use alloc::sync::Arc;
use seele_sys::misc::SystemInfo;
use x86_rtc::Rtc;

use crate::misc::time::Time;
use crate::process::manager::get_current_process;
use crate::systemcall::utils::{SyscallError, SyscallImpl};
use crate::terminal::pty::create_pty;
use crate::thread::misc::with_current_thread;
use crate::thread::yielding::{BlockType, block_current, block_current_with_sig_check};
use crate::{NAME, define_syscall};

const CLOCK_REALTIME: i32 = 0;
const CLOCK_MONOTONIC: i32 = 1;
const CLONE_VM: u64 = 0x0000_0100;
const CLONE_FS: u64 = 0x0000_0200;
const CLONE_FILES: u64 = 0x0000_0400;
const CLONE_SIGHAND: u64 = 0x0000_0800;
const CLONE_THREAD: u64 = 0x0001_0000;
const CLONE_SETTLS: u64 = 0x0008_0000;
const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
const CLONE_CHILD_SETTID: u64 = 0x0100_0000;

#[repr(C)]
struct LinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
struct LinuxUtsname {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

fn write_c_field(dst: &mut [u8], src: &[u8]) {
    let len = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    let len = len.min(dst.len().saturating_sub(1));
    dst[..len].copy_from_slice(&src[..len]);
}

define_syscall!(ClockGettime, |clock_id: i32, tp: u64| {
    let tp = tp as *mut LinuxTimespec;
    if tp.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let ns = match clock_id {
        CLOCK_REALTIME => Time::current().as_nanoseconds() as i64,
        CLOCK_MONOTONIC => Time::since_boot().as_nanoseconds() as i64,
        _ => return Err(SyscallError::InvalidArguments),
    };
    unsafe {
        *tp = LinuxTimespec {
            tv_sec: ns / 1_000_000_000,
            tv_nsec: ns % 1_000_000_000,
        };
    }
    Ok(0)
});

define_syscall!(TimeSinceBoot, {
    Ok(Time::since_boot().as_nanoseconds() as usize)
});

define_syscall!(Uname, |info: u64| {
    let info = info as *mut LinuxUtsname;
    if info.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let sys = SystemInfo::new(NAME, env!("CARGO_PKG_VERSION"));
    unsafe {
        (*info) = LinuxUtsname {
            sysname: [0; 65],
            nodename: [0; 65],
            release: [0; 65],
            version: [0; 65],
            machine: [0; 65],
            domainname: [0; 65],
        };
        write_c_field(&mut (*info).sysname, sys.name());
        write_c_field(&mut (*info).release, sys.version());
        write_c_field(&mut (*info).version, sys.version());
        write_c_field(&mut (*info).machine, b"x86_64");
    }
    Ok(0)
});

define_syscall!(Nanosleep, |req: u64, rem: u64| {
    let req = req as *const LinuxTimespec;
    let rem = rem as *mut LinuxTimespec;
    if req.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let requested = unsafe { &*req };
    if requested.tv_sec < 0 || requested.tv_nsec < 0 || requested.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::InvalidArguments);
    }
    let nanoseconds =
        (requested.tv_sec as u64) * 1_000_000_000 + (requested.tv_nsec as u64);
    let time = Time::since_boot().add_ns(nanoseconds);

    block_current_with_sig_check(BlockType::SetTime(time))?;

    if !rem.is_null() {
        unsafe {
            *rem = LinuxTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
        }
    }

    Ok(0)
});

define_syscall!(Clone, |flags: u64, stack_pointer: u64, parent_tid: u64, child_tid: u64, tls: u64| {
    let required = CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD;
    if (flags & CLONE_THREAD) == 0 || (flags & required) != required {
        return Err(SyscallError::NoSyscall);
    }

    with_current_thread(|thread| {
        let process = get_current_process();
        let thread = thread.clone_and_spawn(process.clone());

        {
            let mut child = thread.lock();
            if stack_pointer != 0 {
                child.snapshot.inner.rsp = stack_pointer;
            }
            child.snapshot.inner.rax = 0;
            if (flags & CLONE_SETTLS) != 0 {
                child.snapshot.fs_base = tls;
            }
        }

        let tid = thread.lock().id.0 as i32;

        unsafe {
            if (flags & CLONE_PARENT_SETTID) != 0 {
                let parent_tid = parent_tid as *mut i32;
                if parent_tid.is_null() {
                    return Err(SyscallError::BadAddress);
                }
                *parent_tid = tid;
            }

            if (flags & CLONE_CHILD_SETTID) != 0 || (flags & CLONE_CHILD_CLEARTID) != 0 {
                let child_tid = child_tid as *mut i32;
                if child_tid.is_null() {
                    return Err(SyscallError::BadAddress);
                }
                *child_tid = tid;
            }
        }

        process.clone().lock().threads.push(Arc::downgrade(&thread));

        Ok(tid as usize)
    })
});

define_syscall!(CreatePty, |master_ptr: *mut i32, slave_ptr: *mut i32| {
    unsafe {
        (*master_ptr, *slave_ptr) = create_pty();
    }
    Ok(0)
});
