use alloc::sync::Arc;
use seele_sys::misc::SystemInfo;
use x86_64::VirtAddr;
use x86_rtc::Rtc;

use crate::memory::addrspace::mem_area::{Data, MemoryArea};
use crate::misc::others::permissions_to_flags;
use crate::misc::time::Time;
use crate::process::manager::get_current_process;
use crate::systemcall::utils::{SyscallError, SyscallImpl};
use crate::terminal::pty::create_pty;
use crate::thread::misc::with_current_thread;
use crate::thread::yielding::{BlockType, block_current, block_current_with_sig_check};
use crate::{NAME, define_syscall};
use seele_sys::permission::Permissions;

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
const RSEQ_FLAG_UNREGISTER: u32 = 1;
const RSEQ_LEN_X86_64: u32 = 32;
const RSEQ_CPU_ID_UNINITIALIZED: u32 = u32::MAX;
const RSEQ_CPU_ID_SINGLE_CORE: u32 = 0;
const GRND_NONBLOCK: u32 = 0x0001;
const GRND_RANDOM: u32 = 0x0002;
const RLIM64_INFINITY: u64 = u64::MAX;
const INITIAL_BRK_RESERVE: u64 = 0x4000_0000;

#[repr(C)]
struct LinuxRlimit64 {
    rlim_cur: u64,
    rlim_max: u64,
}

#[repr(C)]
struct LinuxTimeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
struct LinuxRseq {
    cpu_id_start: u32,
    cpu_id: u32,
    rseq_cs: u64,
    flags: u32,
    _padding: u32,
    _padding2: u64,
}

fn write_rseq_area(rseq_ptr: u64, registered: bool) -> Result<(), SyscallError> {
    if rseq_ptr == 0 {
        return Err(SyscallError::BadAddress);
    }

    let rseq = unsafe { &mut *(rseq_ptr as *mut LinuxRseq) };
    if registered {
        rseq.cpu_id_start = RSEQ_CPU_ID_SINGLE_CORE;
        rseq.cpu_id = RSEQ_CPU_ID_SINGLE_CORE;
        rseq.flags = 0;
    } else {
        rseq.cpu_id_start = RSEQ_CPU_ID_UNINITIALIZED;
        rseq.cpu_id = RSEQ_CPU_ID_UNINITIALIZED;
    }
    Ok(())
}

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

define_syscall!(Gettimeofday, |tv: u64, tz: u64| {
    if tv != 0 {
        let tv = tv as *mut LinuxTimeval;
        if tv.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let now_ns = Time::current().as_nanoseconds() as i64;
        unsafe {
            *tv = LinuxTimeval {
                tv_sec: now_ns / 1_000_000_000,
                tv_usec: (now_ns % 1_000_000_000) / 1_000,
            };
        }
    }

    if tz != 0 {
        let tz = tz as *mut u8;
        if tz.is_null() {
            return Err(SyscallError::BadAddress);
        }
    }

    Ok(0)
});

define_syscall!(Brk, |addr: u64| {
    let process = get_current_process();
    let mut process = process.lock();

    if process.program_break == 0 {
        process.program_break = process
            .addrspace
            .user_mem
            .as_u64()
            .saturating_sub(INITIAL_BRK_RESERVE);
    }

    let current = process.program_break;
    if addr == 0 {
        return Ok(current as usize);
    }

    let old_aligned = current.div_ceil(4096) * 4096;
    let new_aligned = addr.div_ceil(4096) * 4096;

    if new_aligned > old_aligned {
        process.addrspace.register_area(MemoryArea::new(
            VirtAddr::new(old_aligned),
            (new_aligned - old_aligned) / 4096,
            permissions_to_flags(Permissions::READABLE | Permissions::WRITABLE),
            Data::Normal,
            true,
        ));
    } else if new_aligned < old_aligned {
        process
            .addrspace
            .unmap(VirtAddr::new(new_aligned), old_aligned - new_aligned);
    }

    if process.addrspace.user_mem.as_u64() < new_aligned {
        process.addrspace.user_mem = VirtAddr::new(new_aligned);
    }

    process.program_break = addr;
    Ok(addr as usize)
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

define_syscall!(SchedYield, {
    Ok(0)
});

define_syscall!(Getpriority, |_which: i32, _who: i32| {
    Ok(0)
});

define_syscall!(Getresuid, |ruid: u64, euid: u64, suid: u64| {
    for ptr in [ruid, euid, suid] {
        let ptr = ptr as *mut u32;
        if ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        unsafe {
            *ptr = 0;
        }
    }

    Ok(0)
});

define_syscall!(Getresgid, |rgid: u64, egid: u64, sgid: u64| {
    for ptr in [rgid, egid, sgid] {
        let ptr = ptr as *mut u32;
        if ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        unsafe {
            *ptr = 0;
        }
    }

    Ok(0)
});

define_syscall!(Setrlimit, |_resource: i32, _rlimit: u64| {
    Ok(0)
});

define_syscall!(Prlimit64, |pid: i32, _resource: u32, new_limit: u64, old_limit: u64| {
    if pid != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    if new_limit != 0 {
        let new_limit = new_limit as *const LinuxRlimit64;
        if new_limit.is_null() {
            return Err(SyscallError::BadAddress);
        }
    }

    if old_limit != 0 {
        let old_limit = old_limit as *mut LinuxRlimit64;
        if old_limit.is_null() {
            return Err(SyscallError::BadAddress);
        }

        unsafe {
            *old_limit = LinuxRlimit64 {
                rlim_cur: RLIM64_INFINITY,
                rlim_max: RLIM64_INFINITY,
            };
        }
    }

    Ok(0)
});

define_syscall!(Sync, {
    Ok(0)
});

define_syscall!(SetRobustList, |head: u64, len: usize| {
    let current = crate::thread::get_current_thread();
    let mut current = current.lock();
    current.robust_list_head = head;
    current.robust_list_len = len;
    Ok(0)
});

define_syscall!(Rseq, |rseq_ptr: u64, rseq_len: u32, flags: u32, sig: u32| {
    if (flags & !RSEQ_FLAG_UNREGISTER) != 0 || rseq_len != RSEQ_LEN_X86_64 {
        return Err(SyscallError::InvalidArguments);
    }

    let current = crate::thread::get_current_thread();
    let mut current = current.lock();

    if (flags & RSEQ_FLAG_UNREGISTER) != 0 {
        if current.rseq_area != rseq_ptr
            || current.rseq_len != rseq_len
            || current.rseq_sig != sig
        {
            return Err(SyscallError::InvalidArguments);
        }

        write_rseq_area(rseq_ptr, false)?;

        current.rseq_area = 0;
        current.rseq_len = 0;
        current.rseq_flags = 0;
        current.rseq_sig = 0;
        return Ok(0);
    }

    if rseq_ptr == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    if current.rseq_area != 0 {
        return Err(SyscallError::DeviceOrResourceBusy);
    }

    write_rseq_area(rseq_ptr, true)?;

    current.rseq_area = rseq_ptr;
    current.rseq_len = rseq_len;
    current.rseq_flags = flags;
    current.rseq_sig = sig;
    Ok(0)
});

define_syscall!(Getrandom, |buf: u64, len: usize, flags: u32| {
    if (flags & !(GRND_NONBLOCK | GRND_RANDOM)) != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if len == 0 {
        return Ok(0);
    }
    if buf == 0 {
        return Err(SyscallError::BadAddress);
    }

    let out = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, len) };
    let mut state = Time::since_boot().as_nanoseconds()
        ^ Time::current().as_nanoseconds()
        ^ buf.rotate_left(17)
        ^ (len as u64).rotate_left(33);

    for byte in out {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }

    Ok(len)
});

define_syscall!(CreatePty, |master_ptr: *mut i32, slave_ptr: *mut i32| {
    unsafe {
        (*master_ptr, *slave_ptr) = create_pty();
    }
    Ok(0)
});
