use alloc::{sync::Arc, vec::Vec};
use bitflags::bitflags;
use x86_64::VirtAddr;
use x86_rtc::Rtc;

use crate::memory::{
    addrspace::mem_area::{Data, MemoryArea},
    protection::Protection,
    user_safe,
};
use crate::misc::error::AsSyscallError;
use crate::misc::time::{self, Time as KernelTime};
use crate::misc::{others::protection_to_page_flags, utsname::UtsName};
use crate::object::Object;
use crate::object::linux_anon::{
    EventFdObject, InotifyObject, TimerFdObject, wake_linux_io_waiters,
};
use crate::process::manager::{MANAGER, get_current_process};
use crate::signal::{
    action::{SignalAction, SignalHandlingType},
    misc::default_signal_action_vec,
};
use crate::systemcall::utils::{SyscallError, SyscallImpl};
use crate::terminal::pty::create_pty;
use crate::thread::misc::with_current_thread;
use crate::thread::yielding::{BlockType, WakeType, block_current_with_sig_check};
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
    GetKeepCaps = 7,
    SetKeepCaps = 8,
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
            7 => Self::GetKeepCaps,
            8 => Self::SetKeepCaps,
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
const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
const LINUX_CAPABILITY_U32S_3: usize = 2;
const TFD_TIMER_ABSTIME: i32 = 1;
const TFD_NONBLOCK: i32 = 0o4_000;
const EFD_SEMAPHORE: i32 = 0x1;
const EFD_NONBLOCK: i32 = 0o4_000;
const EFD_ALLOWED_FLAGS: i32 = EFD_SEMAPHORE | EFD_NONBLOCK | 0o2_000_000;

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxCapHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxCapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

fn capability_header_targets_current_process(header: &LinuxCapHeader) -> bool {
    header.pid == 0 || header.pid == get_current_process().lock().pid.0 as i32
}

fn current_capability_data() -> [LinuxCapData; LINUX_CAPABILITY_U32S_3] {
    let process = get_current_process();
    let process = process.lock();
    core::array::from_fn(|index| LinuxCapData {
        effective: process.capability_effective[index],
        permitted: process.capability_permitted[index],
        inheritable: process.capability_inheritable[index],
    })
}

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
#[derive(Clone, Copy, Default)]
struct LinuxTimeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxTimezone {
    tz_minuteswest: i32,
    tz_dsttime: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxSchedParam {
    sched_priority: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxSysinfo {
    uptime: i64,
    loads: [u64; 3],
    totalram: u64,
    freeram: u64,
    sharedram: u64,
    bufferram: u64,
    totalswap: u64,
    freeswap: u64,
    procs: u16,
    totalhigh: u64,
    freehigh: u64,
    mem_unit: u32,
    _f: [i8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxItimerval {
    it_interval: LinuxTimeval,
    it_value: LinuxTimeval,
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
#[derive(Clone, Copy, Default)]
struct LinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxItimerspec {
    it_interval: LinuxTimespec,
    it_value: LinuxTimespec,
}

fn linux_timespec_to_ns(timespec: LinuxTimespec) -> Result<u64, SyscallError> {
    if timespec.tv_sec < 0 || timespec.tv_nsec < 0 || timespec.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::InvalidArguments);
    }

    Ok((timespec.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(timespec.tv_nsec as u64))
}

fn ns_to_linux_timespec(ns: u64) -> LinuxTimespec {
    LinuxTimespec {
        tv_sec: (ns / 1_000_000_000) as i64,
        tv_nsec: (ns % 1_000_000_000) as i64,
    }
}

fn linux_timespec_to_realtime_ns(timespec: LinuxTimespec) -> Result<i64, SyscallError> {
    if timespec.tv_sec < 0 || !(0..1_000_000_000).contains(&timespec.tv_nsec) {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(timespec
        .tv_sec
        .saturating_mul(1_000_000_000)
        .saturating_add(timespec.tv_nsec))
}

fn linux_timeval_to_realtime_ns(timeval: LinuxTimeval) -> Result<i64, SyscallError> {
    if timeval.tv_sec < 0 || !(0..1_000_000).contains(&timeval.tv_usec) {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(timeval
        .tv_sec
        .saturating_mul(1_000_000_000)
        .saturating_add(timeval.tv_usec.saturating_mul(1_000)))
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

define_syscall!(ClockSettime, |clock_id: i32, tp: *const LinuxTimespec| {
    if tp.is_null() {
        return Err(SyscallError::BadAddress);
    }

    if !matches!(clock_id, 0 | 8) {
        return Err(SyscallError::InvalidArguments);
    }

    let timespec = user_safe::read(tp)?;
    time::set_unix_timestamp_nanoseconds(linux_timespec_to_realtime_ns(timespec)?);

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

define_syscall!(Capget, |header: *mut LinuxCapHeader,
                         data: *mut LinuxCapData| {
    if header.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut header_value = unsafe { *header };
    if !capability_header_targets_current_process(&header_value) {
        return Err(SyscallError::InvalidArguments);
    }
    header_value.version = LINUX_CAPABILITY_VERSION_3;
    user_safe::write(header, &header_value)?;
    if !data.is_null() {
        user_safe::write(data, &current_capability_data())?;
    }

    Ok(0)
});

define_syscall!(Capset, |header: *const LinuxCapHeader,
                         data: *const LinuxCapData| {
    if header.is_null() || data.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let header_value = unsafe { *header };
    if header_value.version != LINUX_CAPABILITY_VERSION_3 {
        return Err(SyscallError::InvalidArguments);
    }
    if !capability_header_targets_current_process(&header_value) {
        return Err(SyscallError::InvalidArguments);
    }

    let cap_data = unsafe { core::slice::from_raw_parts(data, LINUX_CAPABILITY_U32S_3) };
    let process = get_current_process();
    let mut process = process.lock();
    for (index, caps) in cap_data.iter().enumerate() {
        process.capability_effective[index] = caps.effective;
        process.capability_permitted[index] = caps.permitted;
        process.capability_inheritable[index] = caps.inheritable;
    }

    Ok(0)
});

define_syscall!(InotifyInit, {
    let fd = get_current_process()
        .lock()
        .push_object(Arc::new(InotifyObject::default()));
    Ok(fd)
});

define_syscall!(InotifyInit1, |flags: i32| {
    let object = Arc::new(InotifyObject::default());
    if (flags & TFD_NONBLOCK) != 0 {
        let _ = object.clone().set_flags(crate::object::FileFlags::NONBLOCK);
    }
    let fd = get_current_process().lock().push_object(object);
    Ok(fd)
});

fn create_eventfd(initval: u32, flags: i32) -> Result<usize, SyscallError> {
    if (flags & !EFD_ALLOWED_FLAGS) != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let fd = get_current_process()
        .lock()
        .push_object(EventFdObject::new(initval as u64, flags));
    Ok(fd)
}

define_syscall!(Eventfd, |initval: u32| { create_eventfd(initval, 0) });

define_syscall!(Eventfd2, |initval: u32, flags: i32| {
    create_eventfd(initval, flags)
});

define_syscall!(
    InotifyAddWatch,
    |object: crate::object::misc::ObjectRef, _path: alloc::string::String, _mask: u32| {
        Ok(object.as_inotify()?.add_watch() as usize)
    }
);

define_syscall!(InotifyRmWatch, |object: crate::object::misc::ObjectRef,
                                 _wd: i32| {
    let _ = object.as_inotify()?;
    Ok(0)
});

define_syscall!(TimerfdCreate, |clock_id: i32, flags: i32| {
    if !matches!(clock_id, 0 | 1) {
        return Err(SyscallError::InvalidArguments);
    }

    let object = Arc::new(TimerFdObject::default());
    if (flags & TFD_NONBLOCK) != 0 {
        let _ = object.clone().set_flags(crate::object::FileFlags::NONBLOCK);
    }
    let fd = get_current_process().lock().push_object(object);
    Ok(fd)
});

define_syscall!(
    TimerfdSettime,
    |object: crate::object::misc::ObjectRef,
     flags: i32,
     new_value: *const LinuxItimerspec,
     old_value: *mut LinuxItimerspec| {
        if new_value.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let timerfd = object.as_timerfd()?;
        let now = KernelTime::since_boot();
        let (old_deadline, old_interval_ns) = timerfd.current_timer();
        if !old_value.is_null() {
            let remaining_ns = old_deadline
                .map(|deadline| deadline.sub(now).as_nanoseconds())
                .unwrap_or(0);
            let old_spec = LinuxItimerspec {
                it_interval: ns_to_linux_timespec(old_interval_ns),
                it_value: ns_to_linux_timespec(remaining_ns),
            };
            user_safe::write(old_value, &old_spec)?;
        }

        let new_spec = unsafe { *new_value };
        let value_ns = linux_timespec_to_ns(new_spec.it_value)?;
        let interval_ns = linux_timespec_to_ns(new_spec.it_interval)?;
        let deadline = if value_ns == 0 {
            None
        } else if (flags & TFD_TIMER_ABSTIME) != 0 {
            Some(KernelTime::from_nanoseconds(value_ns))
        } else {
            Some(now.add_ns(value_ns))
        };
        timerfd.set_timer(deadline, interval_ns);
        wake_linux_io_waiters();

        Ok(0)
    }
);

define_syscall!(
    TimerfdGettime,
    |object: crate::object::misc::ObjectRef, curr_value: *mut LinuxItimerspec| {
        if curr_value.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let timerfd = object.as_timerfd()?;
        let now = KernelTime::since_boot();
        let (deadline, interval_ns) = timerfd.current_timer();
        let remaining_ns = deadline
            .map(|deadline| deadline.sub(now).as_nanoseconds())
            .unwrap_or(0);

        let spec = LinuxItimerspec {
            it_interval: ns_to_linux_timespec(interval_ns),
            it_value: ns_to_linux_timespec(remaining_ns),
        };
        user_safe::write(curr_value, &spec)?;

        Ok(0)
    }
);

define_syscall!(TimeSinceBoot, {
    Ok(KernelTime::since_boot().as_nanoseconds() as usize)
});

define_syscall!(Gettimeofday, |tv: *mut LinuxTimeval, tz: *mut LinuxTimezone| {
    if !tv.is_null() {
        let now_ns = KernelTime::current().as_nanoseconds() as i64;
        let timeval = LinuxTimeval {
            tv_sec: now_ns / 1_000_000_000,
            tv_usec: (now_ns % 1_000_000_000) / 1_000,
        };
        user_safe::write(tv, &timeval)?;
    }

    if !tz.is_null() {
        let (tz_minuteswest, tz_dsttime) = time::timezone();
        let timezone = LinuxTimezone {
            tz_minuteswest,
            tz_dsttime,
        };
        user_safe::write(tz, &timezone)?;
    }

    Ok(0)
});

define_syscall!(Settimeofday, |tv: *const LinuxTimeval, tz: *const LinuxTimezone| {
    if !tv.is_null() {
        let timeval = user_safe::read(tv)?;
        time::set_unix_timestamp_nanoseconds(linux_timeval_to_realtime_ns(timeval)?);
    }

    if !tz.is_null() {
        let timezone = user_safe::read(tz)?;
        time::set_timezone(timezone.tz_minuteswest, timezone.tz_dsttime);
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

define_syscall!(Pause, {
    loop {
        match block_current_with_sig_check(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline: None,
        }) {
            Ok(()) => continue,
            Err(err) => return Err(err.as_syscall_error()),
        }
    }
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

define_syscall!(
    Setitimer,
    |which: i32, new_value: *const LinuxItimerval, old_value: *mut LinuxItimerval| {
        if !(0..=2).contains(&which) {
            return Err(SyscallError::InvalidArguments);
        }
        if new_value.is_null() {
            return Err(SyscallError::BadAddress);
        }
        if !old_value.is_null() {
            user_safe::write(old_value, &LinuxItimerval::default())?;
        }
        Ok(0)
    }
);

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

define_syscall!(PidfdOpen, |pid: i32, flags: u32| {
    if pid <= 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if flags != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    Err(SyscallError::NoSyscall)
});

define_syscall!(SchedYield, { Ok(0) });

define_syscall!(Madvise, |_addr: u64, _len: usize, _advice: i32| { Ok(0) });

define_syscall!(Getpriority, |_which: i32, _who: i32| { Ok(0) });

define_syscall!(Setpriority, |_which: i32, _who: i32, _prio: i32| { Ok(0) });

define_syscall!(
    SchedSetscheduler,
    |pid: i32, policy: i32, param: *const LinuxSchedParam| {
        if pid < 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if param.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let param = unsafe { *param };
        if policy < 0 || param.sched_priority < 0 {
            return Err(SyscallError::InvalidArguments);
        }

        Ok(0)
    }
);

define_syscall!(Iopl, |level: i32| {
    if !(0..=3).contains(&level) {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(0)
});

define_syscall!(Ioperm, |_from: u64, _num: u64, _turn_on: i32| { Ok(0) });

define_syscall!(Setgroups, |size: usize, list: *const u32| {
    let groups = if size == 0 {
        Vec::new()
    } else {
        if list.is_null() {
            return Err(SyscallError::BadAddress);
        }
        unsafe { core::slice::from_raw_parts(list, size) }.to_vec()
    };

    get_current_process().lock().supplementary_groups = groups;
    Ok(0)
});

define_syscall!(Getresuid, |ruid: *mut u32,
                            euid: *mut u32,
                            suid: *mut u32| {
    let (real_uid, effective_uid, saved_uid) = {
        let process = get_current_process();
        let process = process.lock();
        (process.real_uid, process.effective_uid, process.saved_uid)
    };
    user_safe::write(ruid, &real_uid)?;
    user_safe::write(euid, &effective_uid)?;
    user_safe::write(suid, &saved_uid)?;

    Ok(0)
});

define_syscall!(Getresgid, |rgid: *mut u32,
                            egid: *mut u32,
                            sgid: *mut u32| {
    let (real_gid, effective_gid, saved_gid) = {
        let process = get_current_process();
        let process = process.lock();
        (process.real_gid, process.effective_gid, process.saved_gid)
    };
    user_safe::write(rgid, &real_gid)?;
    user_safe::write(egid, &effective_gid)?;
    user_safe::write(sgid, &saved_gid)?;

    Ok(0)
});

define_syscall!(Setresuid, |ruid: i32, euid: i32, suid: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    if ruid != -1 {
        process.real_uid = ruid as u32;
    }
    if euid != -1 {
        process.effective_uid = euid as u32;
        process.fs_uid = euid as u32;
    }
    if suid != -1 {
        process.saved_uid = suid as u32;
    }
    Ok(0)
});

define_syscall!(Setresgid, |rgid: i32, egid: i32, sgid: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    if rgid != -1 {
        process.real_gid = rgid as u32;
    }
    if egid != -1 {
        process.effective_gid = egid as u32;
        process.fs_gid = egid as u32;
    }
    if sgid != -1 {
        process.saved_gid = sgid as u32;
    }
    Ok(0)
});

define_syscall!(Getuid, {
    Ok(get_current_process().lock().real_uid as usize)
});

define_syscall!(Getgid, {
    Ok(get_current_process().lock().real_gid as usize)
});

define_syscall!(Setuid, |uid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    process.real_uid = uid;
    process.effective_uid = uid;
    process.saved_uid = uid;
    process.fs_uid = uid;
    Ok(0)
});

define_syscall!(Setgid, |gid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    process.real_gid = gid;
    process.effective_gid = gid;
    process.saved_gid = gid;
    process.fs_gid = gid;
    Ok(0)
});

define_syscall!(Geteuid, {
    Ok(get_current_process().lock().effective_uid as usize)
});

define_syscall!(Getegid, {
    Ok(get_current_process().lock().effective_gid as usize)
});

define_syscall!(Setfsuid, |uid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    let old_uid = process.fs_uid;
    process.fs_uid = uid;
    Ok(old_uid as usize)
});

define_syscall!(Setfsgid, |gid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    let old_gid = process.fs_gid;
    process.fs_gid = gid;
    Ok(old_gid as usize)
});

define_syscall!(Time, |time_ptr: *mut i64| {
    let seconds = (KernelTime::current().as_nanoseconds() / 1_000_000_000) as i64;
    if !time_ptr.is_null() {
        user_safe::write(time_ptr, &seconds)?;
    }
    Ok(seconds as usize)
});

define_syscall!(Sysinfo, |info_ptr: *mut LinuxSysinfo| {
    let uptime = (KernelTime::since_boot().as_nanoseconds() / 1_000_000_000) as i64;
    let info = LinuxSysinfo {
        uptime,
        totalram: 4 * 1024 * 1024 * 1024,
        freeram: 2 * 1024 * 1024 * 1024,
        procs: 1,
        mem_unit: 1,
        ..Default::default()
    };
    user_safe::write(info_ptr, &info)?;
    Ok(0)
});

define_syscall!(
    SchedSetaffinity,
    |pid: i32, cpusetsize: usize, mask_ptr: *const u8| {
        if pid < 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if cpusetsize == 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if mask_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        Ok(0)
    }
);

define_syscall!(
    SchedGetaffinity,
    |pid: i32, cpusetsize: usize, mask_ptr: *mut u8| {
        if pid < 0 {
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

define_syscall!(SchedRrGetInterval, |pid: i32, tp: *mut LinuxTimespec| {
    if pid < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if tp.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let timespec = LinuxTimespec {
        tv_sec: 0,
        tv_nsec: 100_000_000,
    };
    user_safe::write(tp, &timespec)?;
    Ok(0)
});

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
        PrctlOption::SetKeepCaps => {
            if arg2 > 1 {
                return Err(SyscallError::InvalidArguments);
            }
            get_current_process().lock().keep_capabilities = arg2 != 0;
            Ok(0)
        }
        PrctlOption::GetPdeathsig => {
            user_safe::write(arg2 as *mut u8, &0i32)?;
            Ok(0)
        }
        PrctlOption::GetDumpable | PrctlOption::GetNoNewPrivs => Ok(0),
        PrctlOption::GetKeepCaps => Ok(get_current_process().lock().keep_capabilities as usize),
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
