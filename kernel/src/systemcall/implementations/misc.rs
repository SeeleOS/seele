use alloc::{sync::Arc, vec::Vec};
use bitflags::bitflags;
use x86_64::VirtAddr;
use x86_rtc::Rtc;

use crate::memory::{
    addrspace::mem_area::{Data, MemoryArea},
    protection::Protection,
    user_safe,
};
use crate::misc::time::Time as KernelTime;
use crate::misc::{others::protection_to_page_flags, utsname::UtsName};
use crate::process::manager::{MANAGER, get_current_process};
use crate::signal::{
    action::{SignalAction, SignalHandlingType},
    misc::default_signal_action_vec,
};
use crate::systemcall::utils::{SyscallError, SyscallImpl};
use crate::terminal::pty::create_pty;
use crate::thread::misc::with_current_thread;
use crate::thread::yielding::{BlockType, block_current, block_current_with_sig_check};
use crate::{NAME, define_syscall};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct CloneFlags: u64 {
        const VM = 0x0000_0100;
        const FS = 0x0000_0200;
        const FILES = 0x0000_0400;
        const SIGHAND = 0x0000_0800;
        const VFORK = 0x0000_4000;
        const THREAD = 0x0001_0000;
        const SETTLS = 0x0008_0000;
        const PARENT_SETTID = 0x0010_0000;
        const CHILD_CLEARTID = 0x0020_0000;
        const CHILD_SETTID = 0x0100_0000;
        const CLEAR_SIGHAND = 0x1_0000_0000;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxCloneArgs {
    flags: u64,
    pidfd: u64,
    child_tid: u64,
    parent_tid: u64,
    exit_signal: u64,
    stack: u64,
    stack_size: u64,
    tls: u64,
    set_tid: u64,
    set_tid_size: u64,
    cgroup: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
enum PrctlOption {
    SetPdeathsig = 1,
    GetPdeathsig = 2,
    GetDumpable = 3,
    SetDumpable = 4,
    SetName = 15,
    GetName = 16,
    CapbsetRead = 23,
    SetNoNewPrivs = 38,
    GetNoNewPrivs = 39,
}

impl TryFrom<i32> for PrctlOption {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::SetPdeathsig,
            2 => Self::GetPdeathsig,
            3 => Self::GetDumpable,
            4 => Self::SetDumpable,
            15 => Self::SetName,
            16 => Self::GetName,
            23 => Self::CapbsetRead,
            38 => Self::SetNoNewPrivs,
            39 => Self::GetNoNewPrivs,
            _ => return Err(()),
        })
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct RseqFlags: u32 {
        const UNREGISTER = 1;
    }
}

const RSEQ_LEN_X86_64: u32 = 32;
const RSEQ_CPU_ID_UNINITIALIZED: u32 = u32::MAX;
const RSEQ_CPU_ID_SINGLE_CORE: u32 = 0;
const RLIM64_INFINITY: u64 = u64::MAX;
const RLIMIT_NOFILE_DEFAULT: u64 = 1024;
const INITIAL_BRK_RESERVE: u64 = 0x4000_0000;
const TIMER_ABSTIME: i32 = 1;

fn clone_cleared_signal_actions(old_actions: &[SignalAction]) -> Vec<SignalAction> {
    let defaults = default_signal_action_vec();
    old_actions
        .iter()
        .zip(defaults)
        .map(|(old, default)| match old.handling_type {
            SignalHandlingType::Ignore => old.clone(),
            SignalHandlingType::Default
            | SignalHandlingType::Function1(_)
            | SignalHandlingType::Function2(_) => default,
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum RlimitResource {
    NoFile = 7,
}

impl TryFrom<u32> for RlimitResource {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            7 => Self::NoFile,
            _ => return Err(()),
        })
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct GetRandomFlags: u32 {
        const NONBLOCK = 0x0001;
        const RANDOM = 0x0002;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxRlimit64 {
    rlim_cur: u64,
    rlim_max: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxTimeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxRseq {
    cpu_id_start: u32,
    cpu_id: u32,
    rseq_cs: u64,
    flags: u32,
    _padding: u32,
    _padding2: u64,
}

fn write_rseq_area(rseq_ptr: *mut LinuxRseq, registered: bool) -> Result<(), SyscallError> {
    if rseq_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut rseq = LinuxRseq {
        cpu_id_start: RSEQ_CPU_ID_UNINITIALIZED,
        cpu_id: RSEQ_CPU_ID_UNINITIALIZED,
        rseq_cs: 0,
        flags: 0,
        _padding: 0,
        _padding2: 0,
    };
    if registered {
        rseq.cpu_id_start = RSEQ_CPU_ID_SINGLE_CORE;
        rseq.cpu_id = RSEQ_CPU_ID_SINGLE_CORE;
    }
    user_safe::write(rseq_ptr, &rseq)?;
    Ok(())
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn linux_clock_now_ns(clock_id: i32) -> Result<i64, SyscallError> {
    match clock_id {
        0 | 5 | 8 | 11 => Ok(KernelTime::current().as_nanoseconds() as i64),
        1 | 4 | 6 | 7 | 9 => Ok(KernelTime::since_boot().as_nanoseconds() as i64),
        2 | 3 => Ok(0),
        _ => Err(SyscallError::InvalidArguments),
    }
}

define_syscall!(ClockGettime, |clock_id: i32, tp: *mut LinuxTimespec| {
    if tp.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let ns = linux_clock_now_ns(clock_id)?;
    let timespec = LinuxTimespec {
        tv_sec: ns / 1_000_000_000,
        tv_nsec: ns % 1_000_000_000,
    };
    user_safe::write(tp, &timespec)?;
    Ok(0)
});

define_syscall!(ClockGetres, |clock_id: i32, tp: *mut LinuxTimespec| {
    let _ = linux_clock_now_ns(clock_id)?;

    if tp.is_null() {
        return Ok(0);
    }

    let timespec = LinuxTimespec {
        tv_sec: 0,
        tv_nsec: 1,
    };
    user_safe::write(tp, &timespec)?;

    Ok(0)
});

define_syscall!(TimeSinceBoot, {
    Ok(KernelTime::since_boot().as_nanoseconds() as usize)
});

define_syscall!(Gettimeofday, |tv: *mut LinuxTimeval, tz: *mut u8| {
    if !tv.is_null() {
        let now_ns = KernelTime::current().as_nanoseconds() as i64;
        let timeval = LinuxTimeval {
            tv_sec: now_ns / 1_000_000_000,
            tv_usec: (now_ns % 1_000_000_000) / 1_000,
        };
        user_safe::write(tv, &timeval)?;
    }

    if !tz.is_null() {
        if tz.is_null() {
            return Err(SyscallError::BadAddress);
        }
    }

    Ok(0)
});

define_syscall!(Umask, |mask: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    let old_mask = process.file_mode_creation_mask;
    process.file_mode_creation_mask = mask & 0o777;
    Ok(old_mask as usize)
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
            protection_to_page_flags(Protection::READ | Protection::WRITE),
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

define_syscall!(Uname, |info: *mut UtsName| {
    if info.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let uts = UtsName::new(
        NAME,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        "x86_64",
    );
    user_safe::write(info, &uts)?;
    Ok(0)
});

define_syscall!(
    Nanosleep,
    |req: *const LinuxTimespec, rem: *mut LinuxTimespec| {
        if req.is_null() {
            return Err(SyscallError::BadAddress);
        }
        let requested = unsafe { &*req };
        if requested.tv_sec < 0 || requested.tv_nsec < 0 || requested.tv_nsec >= 1_000_000_000 {
            return Err(SyscallError::InvalidArguments);
        }
        let nanoseconds = (requested.tv_sec as u64) * 1_000_000_000 + (requested.tv_nsec as u64);
        let time = KernelTime::since_boot().add_ns(nanoseconds);

        block_current_with_sig_check(BlockType::SetTime(time))?;

        if !rem.is_null() {
            let remaining = LinuxTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            user_safe::write(rem, &remaining)?;
        }

        Ok(0)
    }
);

define_syscall!(Alarm, |_seconds: u32| { Ok(0) });

define_syscall!(RtSigsuspend, |mask: *const u64, sigset_size: usize| {
    if sigset_size != 8 {
        return Err(SyscallError::InvalidArguments);
    }

    if mask.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let new_mask = crate::signal::Signals::from_bits_truncate(unsafe { *mask });
    let old_mask = {
        let current = crate::thread::get_current_thread();
        let mut current = current.lock();
        let old = current.blocked_signals;
        current.blocked_signals = new_mask;
        old
    };

    loop {
        let result = block_current_with_sig_check(BlockType::WakeRequired {
            wake_type: crate::thread::yielding::WakeType::IO,
            deadline: None,
        });

        if result.is_err() {
            crate::thread::get_current_thread().lock().blocked_signals = old_mask;
            return Err(SyscallError::Interrupted);
        }
    }
});

define_syscall!(
    ClockNanosleep,
    |clock_id: i32, flags: i32, req: *const LinuxTimespec, rem: *mut LinuxTimespec| {
        if req.is_null() {
            return Err(SyscallError::BadAddress);
        }
        if (flags & !TIMER_ABSTIME) != 0 {
            return Err(SyscallError::InvalidArguments);
        }

        let requested = unsafe { &*req };
        if requested.tv_sec < 0 || requested.tv_nsec < 0 || requested.tv_nsec >= 1_000_000_000 {
            return Err(SyscallError::InvalidArguments);
        }

        let clock = crate::misc::timer::ClockId::try_from(clock_id as u64)
            .map_err(|_| SyscallError::InvalidArguments)?;
        let requested_ns =
            (requested.tv_sec as u64).saturating_mul(1_000_000_000) + (requested.tv_nsec as u64);

        let now = match clock {
            crate::misc::timer::ClockId::Realtime => KernelTime::current(),
            crate::misc::timer::ClockId::SinceBoot => KernelTime::since_boot(),
        };
        let deadline = if (flags & TIMER_ABSTIME) != 0 {
            KernelTime::from_nanoseconds(requested_ns)
        } else {
            now.add_ns(requested_ns)
        };

        if deadline > now {
            block_current_with_sig_check(BlockType::SetTime(deadline))?;
        }

        if !rem.is_null() {
            let remaining = LinuxTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            user_safe::write(rem, &remaining)?;
        }

        Ok(0)
    }
);

define_syscall!(Clone, |flags: u64,
                        stack_pointer: u64,
                        parent_tid: *mut i32,
                        child_tid: *mut i32,
                        tls: u64| {
    let clone_flags = CloneFlags::from_bits_truncate(flags);
    let exit_signal = (flags & 0xff) as u8;
    let required = CloneFlags::VM
        | CloneFlags::FS
        | CloneFlags::FILES
        | CloneFlags::SIGHAND
        | CloneFlags::THREAD;
    if !clone_flags.contains(CloneFlags::THREAD) {
        let unsupported = flags
            & !(0xff
                | CloneFlags::VM.bits()
                | CloneFlags::VFORK.bits()
                | CloneFlags::CLEAR_SIGHAND.bits()
                | CloneFlags::PARENT_SETTID.bits()
                | CloneFlags::CHILD_SETTID.bits()
                | CloneFlags::CHILD_CLEARTID.bits()
                | CloneFlags::SETTLS.bits());
        if unsupported != 0 || (exit_signal != 0 && exit_signal != 17) {
            return Err(SyscallError::NoSyscall);
        }

        let current = get_current_process();
        let (child_process, child_thread) = crate::process::Process::fork(current.clone());
        let pid = child_process.lock().pid;
        MANAGER.lock().processes.insert(pid, child_process.clone());

        if clone_flags.contains(CloneFlags::CLEAR_SIGHAND) {
            let mut child = child_process.lock();
            child.signal_actions = clone_cleared_signal_actions(&child.signal_actions);
        }

        {
            let mut child = child_thread.lock();
            if stack_pointer != 0 {
                child.snapshot.inner.rsp = stack_pointer;
            }
            child.snapshot.inner.rax = 0;
            if clone_flags.contains(CloneFlags::SETTLS) {
                child.snapshot.fs_base = tls;
            }
        }

        if clone_flags.contains(CloneFlags::PARENT_SETTID) {
            user_safe::write(parent_tid, &(pid.0 as i32))?;
        }

        if clone_flags.intersects(CloneFlags::CHILD_SETTID | CloneFlags::CHILD_CLEARTID) {
            child_process
                .lock()
                .addrspace
                .write(child_tid, &(pid.0 as i32))?;
        }

        if clone_flags.contains(CloneFlags::CHILD_CLEARTID) {
            child_thread.lock().clear_child_tid = child_tid as u64;
        }

        return Ok(pid.0 as usize);
    }

    let flags = clone_flags;
    if !flags.contains(required) {
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
            if flags.contains(CloneFlags::SETTLS) {
                child.snapshot.fs_base = tls;
            }
            if flags.contains(CloneFlags::CHILD_CLEARTID) {
                child.clear_child_tid = child_tid as u64;
            }
        }

        let tid = thread.lock().id.0 as i32;
        crate::s_println!(
            "clone thread: pid={} tid={} flags={:#x} child_tid={:#x}",
            process.lock().pid.0,
            tid,
            flags.bits(),
            child_tid as usize
        );

        if flags.contains(CloneFlags::PARENT_SETTID) {
            user_safe::write(parent_tid, &tid)?;
        }

        if flags.intersects(CloneFlags::CHILD_SETTID | CloneFlags::CHILD_CLEARTID) {
            user_safe::write(child_tid, &tid)?;
        }

        process.clone().lock().threads.push(Arc::downgrade(&thread));

        Ok(tid as usize)
    })
});

define_syscall!(Clone3, |args: *const LinuxCloneArgs, size: usize| {
    if size < core::mem::size_of::<LinuxCloneArgs>() {
        return Err(SyscallError::InvalidArguments);
    }
    if args.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let args = unsafe { &*args };
    if args.pidfd != 0 || args.set_tid != 0 || args.set_tid_size != 0 || args.cgroup != 0 {
        return Err(SyscallError::NoSyscall);
    }

    let stack_pointer = if args.stack == 0 {
        0
    } else {
        args.stack.saturating_add(args.stack_size)
    };
    let flags = args.flags | (args.exit_signal & 0xff);

    <Clone as SyscallImpl>::handle_call(
        flags,
        stack_pointer,
        args.parent_tid,
        args.child_tid,
        args.tls,
        0,
    )
});

define_syscall!(SchedYield, { Ok(0) });

define_syscall!(Madvise, |_addr: u64, _len: usize, _advice: i32| { Ok(0) });

define_syscall!(Getpriority, |_which: i32, _who: i32| { Ok(0) });

define_syscall!(Setpriority, |_which: i32, _who: i32, _prio: i32| { Ok(0) });

define_syscall!(Iopl, |level: i32| {
    if !(0..=3).contains(&level) {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(0)
});

define_syscall!(Ioperm, |_from: u64, _num: u64, _turn_on: i32| { Ok(0) });

define_syscall!(Getresuid, |ruid: *mut u32,
                            euid: *mut u32,
                            suid: *mut u32| {
    for ptr in [ruid, euid, suid] {
        user_safe::write(ptr, &0u32)?;
    }

    Ok(0)
});

define_syscall!(Getresgid, |rgid: *mut u32,
                            egid: *mut u32,
                            sgid: *mut u32| {
    for ptr in [rgid, egid, sgid] {
        user_safe::write(ptr, &0u32)?;
    }

    Ok(0)
});

define_syscall!(Getuid, { Ok(0) });

define_syscall!(Getgid, { Ok(0) });

define_syscall!(Setuid, |_uid: u32| { Ok(0) });

define_syscall!(Setgid, |_gid: u32| { Ok(0) });

define_syscall!(Geteuid, { Ok(0) });

define_syscall!(Getegid, { Ok(0) });

define_syscall!(Setfsuid, |_uid: u32| { Ok(0) });

define_syscall!(Setfsgid, |_gid: u32| { Ok(0) });

define_syscall!(Time, |time_ptr: *mut i64| {
    let seconds = (KernelTime::current().as_nanoseconds() / 1_000_000_000) as i64;
    if !time_ptr.is_null() {
        user_safe::write(time_ptr, &seconds)?;
    }
    Ok(seconds as usize)
});

define_syscall!(
    SchedGetaffinity,
    |pid: i32, cpusetsize: usize, mask_ptr: *mut u8| {
        if pid != 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if cpusetsize < core::mem::size_of::<usize>() {
            return Err(SyscallError::InvalidArguments);
        }

        let mut mask = Vec::with_capacity(cpusetsize);
        mask.resize(cpusetsize, 0);
        mask[0] = 1;
        user_safe::write(mask_ptr, &mask)?;

        Ok(core::mem::size_of::<usize>())
    }
);

define_syscall!(Setrlimit, |_resource: i32, _rlimit: u64| { Ok(0) });

define_syscall!(Prctl, |option: i32,
                        arg2: u64,
                        _arg3: u64,
                        _arg4: u64,
                        _arg5: u64| {
    match PrctlOption::try_from(option).map_err(|_| SyscallError::InvalidArguments)? {
        PrctlOption::SetPdeathsig
        | PrctlOption::SetDumpable
        | PrctlOption::SetName
        | PrctlOption::SetNoNewPrivs => Ok(0),
        PrctlOption::GetPdeathsig => {
            user_safe::write(arg2 as *mut u8, &0i32)?;
            Ok(0)
        }
        PrctlOption::GetDumpable | PrctlOption::GetNoNewPrivs => Ok(0),
        PrctlOption::GetName => {
            let name = b"main\0";
            let mut buffer = [0u8; 16];
            buffer[..name.len()].copy_from_slice(name);
            user_safe::write(arg2 as *mut u8, &buffer)?;
            Ok(0)
        }
        PrctlOption::CapbsetRead => Ok(0),
    }
});

define_syscall!(
    Prlimit64,
    |pid: i32, resource: u32, new_limit: *const LinuxRlimit64, old_limit: *mut LinuxRlimit64| {
        if pid != 0 {
            return Err(SyscallError::InvalidArguments);
        }

        if !new_limit.is_null() {
            if new_limit.is_null() {
                return Err(SyscallError::BadAddress);
            }
        }

        if !old_limit.is_null() {
            let (rlim_cur, rlim_max) = match RlimitResource::try_from(resource) {
                Ok(RlimitResource::NoFile) => (RLIMIT_NOFILE_DEFAULT, RLIMIT_NOFILE_DEFAULT),
                Err(_) => (RLIM64_INFINITY, RLIM64_INFINITY),
            };
            let limit = LinuxRlimit64 { rlim_cur, rlim_max };
            user_safe::write(old_limit, &limit)?;
        }

        Ok(0)
    }
);

define_syscall!(Sync, { Ok(0) });

define_syscall!(SetRobustList, |head: u64, len: usize| {
    let current = crate::thread::get_current_thread();
    let mut current = current.lock();
    current.robust_list_head = head;
    current.robust_list_len = len;
    Ok(0)
});

define_syscall!(Rseq, |rseq_ptr: *mut LinuxRseq,
                       rseq_len: u32,
                       flags: u32,
                       sig: u32| {
    let flags = RseqFlags::from_bits_truncate(flags);
    if flags.bits() != flags.bits() & RseqFlags::UNREGISTER.bits() || rseq_len != RSEQ_LEN_X86_64 {
        return Err(SyscallError::InvalidArguments);
    }

    let current = crate::thread::get_current_thread();
    let mut current = current.lock();

    if flags.contains(RseqFlags::UNREGISTER) {
        if current.rseq_area != rseq_ptr as u64
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

    if rseq_ptr.is_null() {
        return Err(SyscallError::InvalidArguments);
    }

    if current.rseq_area != 0 {
        return Err(SyscallError::DeviceOrResourceBusy);
    }

    write_rseq_area(rseq_ptr, true)?;

    current.rseq_area = rseq_ptr as u64;
    current.rseq_len = rseq_len;
    current.rseq_flags = flags.bits();
    current.rseq_sig = sig;
    Ok(0)
});

define_syscall!(Getrandom, |buf: *mut u8, len: usize, flags: u32| {
    let flags = GetRandomFlags::from_bits_truncate(flags);
    if flags.bits() != flags.bits() & (GetRandomFlags::NONBLOCK | GetRandomFlags::RANDOM).bits() {
        return Err(SyscallError::InvalidArguments);
    }
    if len == 0 {
        return Ok(0);
    }
    if buf.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut state = KernelTime::since_boot().as_nanoseconds()
        ^ KernelTime::current().as_nanoseconds()
        ^ (buf as u64).rotate_left(17)
        ^ (len as u64).rotate_left(33);
    let mut out = Vec::with_capacity(len);
    out.resize(len, 0);

    for byte in &mut out {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }

    user_safe::write(buf, &out[..])?;

    Ok(len)
});

define_syscall!(CreatePty, |master_ptr: *mut i32, slave_ptr: *mut i32| {
    if master_ptr.is_null() || slave_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let (master, slave) = create_pty();
    user_safe::write(master_ptr, &master)?;
    user_safe::write(slave_ptr, &slave)?;
    Ok(0)
});
