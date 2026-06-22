use bitflags::bitflags;

use crate::misc::{
    time::{self, Time as KernelTime},
    timer::{ClockId, TimerNotifyMethod, TimerState},
};
use crate::object::FileFlags;
use crate::object::linux_anon::{TimerFdObject, wake_linux_io_waiters};
use crate::object::misc::ObjectRef;
use crate::process::{FdFlags, manager::get_current_process};
use crate::systemcall::utils::{SyscallError, SyscallImpl};
use crate::thread::yielding::{BlockType, block_current_with_sig_check};
use crate::{define_syscall, memory::user_safe};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TimerFdFlags: i32 {
        const TFD_NONBLOCK = 0o4_000;
        const TFD_CLOEXEC = 0o2_000_000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TimerSetTimeFlags: i32 {
        const TFD_TIMER_ABSTIME = 1;
        const TFD_TIMER_CANCEL_ON_SET = 2;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct ClockNanosleepFlags: i32 {
        const TIMER_ABSTIME = 1;
    }
}

#[derive(Clone, Copy)]
enum SleepClock {
    Realtime,
    Monotonic,
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
    _pad: u16,
    totalhigh: u64,
    freehigh: u64,
    mem_unit: u32,
    _f: [i8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxItimerval {
    it_interval: LinuxTimeval,
    it_value: LinuxTimeval,
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

fn linux_timeval_to_ns(timeval: LinuxTimeval) -> Result<u64, SyscallError> {
    if timeval.tv_sec < 0 || !(0..1_000_000).contains(&timeval.tv_usec) {
        return Err(SyscallError::InvalidArguments);
    }

    Ok((timeval.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(timeval.tv_usec as u64 * 1_000))
}

fn ns_to_linux_timeval(ns: u64) -> LinuxTimeval {
    LinuxTimeval {
        tv_sec: (ns / 1_000_000_000) as i64,
        tv_usec: ((ns % 1_000_000_000) / 1_000) as i64,
    }
}

fn itimer_clock(which: i32) -> Result<ClockId, SyscallError> {
    match which {
        0 => Ok(ClockId::SinceBoot),
        1 | 2 => Ok(ClockId::SinceBoot),
        _ => Err(SyscallError::InvalidArguments),
    }
}

fn itimer_signal(which: i32) -> Result<crate::signal::Signal, SyscallError> {
    match which {
        0 => Ok(crate::signal::Signal::SIGALRM),
        1 => Ok(crate::signal::Signal::SIGVTALRM),
        2 => Ok(crate::signal::Signal::SIGPROF),
        _ => Err(SyscallError::InvalidArguments),
    }
}

fn itimer_to_linux_value(state: TimerState, clock: ClockId) -> LinuxItimerval {
    let now = match clock {
        ClockId::Realtime => KernelTime::current(),
        ClockId::SinceBoot => KernelTime::since_boot(),
    };
    match state {
        TimerState::Disabled => LinuxItimerval::default(),
        TimerState::OneShot { deadline } => LinuxItimerval {
            it_interval: LinuxTimeval::default(),
            it_value: ns_to_linux_timeval(deadline.sub(now).as_nanoseconds()),
        },
        TimerState::Periodic { deadline, interval } => LinuxItimerval {
            it_interval: ns_to_linux_timeval(interval.as_nanoseconds()),
            it_value: ns_to_linux_timeval(deadline.sub(now).as_nanoseconds()),
        },
    }
}

fn linux_clock_now_ns(clock_id: i32) -> Result<i64, SyscallError> {
    match clock_id {
        0 | 5 | 8 | 11 => Ok(KernelTime::current().as_nanoseconds() as i64),
        1 | 4 | 6 | 7 | 9 => Ok(KernelTime::since_boot().as_nanoseconds() as i64),
        2 | 3 => Ok(KernelTime::since_boot().as_nanoseconds().max(1) as i64),
        _ => Err(SyscallError::InvalidArguments),
    }
}

fn clock_nanosleep_clock(clock_id: i32) -> Result<SleepClock, SyscallError> {
    match clock_id {
        0 | 5 | 8 | 11 => Ok(SleepClock::Realtime),
        1 | 4 | 6 | 7 | 9 => Ok(SleepClock::Monotonic),
        2 | 3 => Err(SyscallError::OperationNotSupported),
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

define_syscall!(TimerfdCreate, |clock_id: i32, flags: TimerFdFlags| {
    if !matches!(clock_id, 0 | 1) {
        return Err(SyscallError::InvalidArguments);
    }

    let file_flags = if flags.contains(TimerFdFlags::TFD_NONBLOCK) {
        FileFlags::NONBLOCK
    } else {
        FileFlags::empty()
    };
    let object = TimerFdObject::new(file_flags);
    let fd_flags = if flags.contains(TimerFdFlags::TFD_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(object, fd_flags);
    Ok(fd)
});

define_syscall!(
    TimerfdSettime,
    |object: ObjectRef,
     flags: TimerSetTimeFlags,
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

        let new_spec = user_safe::read(new_value)?;
        let value_ns = linux_timespec_to_ns(new_spec.it_value)?;
        let interval_ns = linux_timespec_to_ns(new_spec.it_interval)?;
        let deadline = if value_ns == 0 {
            None
        } else if flags.contains(TimerSetTimeFlags::TFD_TIMER_ABSTIME) {
            Some(KernelTime::from_nanoseconds(value_ns))
        } else {
            Some(now.add_ns(value_ns))
        };
        timerfd.set_timer(deadline, interval_ns);
        wake_linux_io_waiters();
        if timerfd.is_read_ready() {
            timerfd.wake_waiters();
        }

        Ok(0)
    }
);

define_syscall!(
    TimerfdGettime,
    |object: ObjectRef, curr_value: *mut LinuxItimerspec| {
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

define_syscall!(
    Gettimeofday,
    |tv: *mut LinuxTimeval, tz: *mut LinuxTimezone| {
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
    }
);

define_syscall!(
    Settimeofday,
    |tv: *const LinuxTimeval, tz: *const LinuxTimezone| {
        if !tv.is_null() {
            let timeval = user_safe::read(tv)?;
            time::set_unix_timestamp_nanoseconds(linux_timeval_to_realtime_ns(timeval)?);
        }

        if !tz.is_null() {
            let timezone = user_safe::read(tz)?;
            time::set_timezone(timezone.tz_minuteswest, timezone.tz_dsttime);
        }

        Ok(0)
    }
);

define_syscall!(
    Nanosleep,
    |req: *const LinuxTimespec, rem: *mut LinuxTimespec| {
        if req.is_null() {
            return Err(SyscallError::BadAddress);
        }
        let requested = user_safe::read(req)?;
        if requested.tv_sec < 0 || requested.tv_nsec < 0 || requested.tv_nsec >= 1_000_000_000 {
            return Err(SyscallError::InvalidArguments);
        }
        let nanoseconds = (requested.tv_sec as u64) * 1_000_000_000 + (requested.tv_nsec as u64);
        let time = KernelTime::since_boot().add_ns(nanoseconds);

        if time > KernelTime::since_boot() {
            block_current_with_sig_check(BlockType::SetTime(time))?;
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

define_syscall!(
    Setitimer,
    |which: i32, new_value: *const LinuxItimerval, old_value: *mut LinuxItimerval| {
        let clock = itimer_clock(which)?;
        let requested = if new_value.is_null() {
            None
        } else {
            Some(user_safe::read(new_value)?)
        };
        let process = get_current_process();
        let mut process = process.lock();

        let timer_id = which as usize;
        if !old_value.is_null() {
            let old = process
                .timers
                .get(timer_id)
                .and_then(Option::as_ref)
                .map(|timer| itimer_to_linux_value(timer.state, timer.time_type))
                .unwrap_or_default();
            process.addrspace.write(old_value, &old)?;
        }

        let Some(requested) = requested else {
            return Ok(0);
        };

        let value_ns = linux_timeval_to_ns(requested.it_value)?;
        let interval_ns = linux_timeval_to_ns(requested.it_interval)?;
        if process.timers.len() <= timer_id {
            process.timers.resize_with(timer_id + 1, || None);
        }
        let state = if value_ns == 0 {
            TimerState::Disabled
        } else {
            let now = match clock {
                ClockId::Realtime => KernelTime::current(),
                ClockId::SinceBoot => KernelTime::since_boot(),
            };
            let deadline = now.add_ns(value_ns);
            if interval_ns == 0 {
                TimerState::OneShot { deadline }
            } else {
                TimerState::Periodic {
                    deadline,
                    interval: KernelTime::from_nanoseconds(interval_ns),
                }
            }
        };
        process.timers[timer_id] = Some(crate::misc::timer::Timer {
            notify_method: TimerNotifyMethod::Signal(itimer_signal(which)?),
            time_type: clock,
            state,
            overrun: 0,
        });
        Ok(0)
    }
);

define_syscall!(
    Getitimer,
    |which: i32, current_value: *mut LinuxItimerval| {
        if current_value.is_null() {
            return Err(SyscallError::BadAddress);
        }
        let clock = itimer_clock(which)?;
        let process = get_current_process();
        let mut process = process.lock();
        let timer_id = which as usize;
        let value = process
            .timers
            .get(timer_id)
            .and_then(Option::as_ref)
            .map(|timer| itimer_to_linux_value(timer.state, timer.time_type))
            .unwrap_or_else(|| itimer_to_linux_value(TimerState::Disabled, clock));
        process.addrspace.write(current_value, &value)?;
        Ok(0)
    }
);

define_syscall!(
    ClockNanosleep,
    |clock_id: i32,
     flags: ClockNanosleepFlags,
     req: *const LinuxTimespec,
     rem: *mut LinuxTimespec| {
        if req.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let requested = user_safe::read(req)?;
        if requested.tv_sec < 0 || requested.tv_nsec < 0 || requested.tv_nsec >= 1_000_000_000 {
            return Err(SyscallError::InvalidArguments);
        }
        let clock = clock_nanosleep_clock(clock_id)?;
        let requested_ns =
            (requested.tv_sec as u64).saturating_mul(1_000_000_000) + (requested.tv_nsec as u64);

        let deadline = if flags.contains(ClockNanosleepFlags::TIMER_ABSTIME) {
            match clock {
                SleepClock::Realtime => {
                    let now_realtime = KernelTime::current();
                    let now_boot = KernelTime::since_boot();
                    now_boot.add_ns(requested_ns.saturating_sub(now_realtime.as_nanoseconds()))
                }
                SleepClock::Monotonic => KernelTime::from_nanoseconds(requested_ns),
            }
        } else {
            // Blocked-thread timeouts are evaluated against since_boot.
            // Relative sleeps are duration-based, so normalize them onto
            // that clock domain even for CLOCK_REALTIME.
            KernelTime::since_boot().add_ns(requested_ns)
        };

        if deadline > KernelTime::since_boot()
            && let Err(err) = block_current_with_sig_check(BlockType::SetTime(deadline))
        {
            if !flags.contains(ClockNanosleepFlags::TIMER_ABSTIME) && !rem.is_null() {
                let remaining = deadline.sub(KernelTime::since_boot()).as_nanoseconds();
                user_safe::write(rem, &ns_to_linux_timespec(remaining))?;
            }
            return Err(err.into());
        }

        if !flags.contains(ClockNanosleepFlags::TIMER_ABSTIME) && !rem.is_null() {
            let remaining = LinuxTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            user_safe::write(rem, &remaining)?;
        }

        Ok(0)
    }
);

define_syscall!(Time, |time_ptr: *mut i64| {
    let seconds = (KernelTime::current().as_nanoseconds() / 1_000_000_000) as i64;
    if !time_ptr.is_null() {
        user_safe::write(time_ptr, &seconds)?;
    }
    Ok(seconds as usize)
});

define_syscall!(Sysinfo, |info_ptr: *mut LinuxSysinfo| {
    let uptime = (KernelTime::since_boot().as_nanoseconds() / 1_000_000_000) as i64;
    let totalram = crate::memory::usable_memory_bytes();
    let info = LinuxSysinfo {
        uptime,
        totalram,
        freeram: totalram,
        procs: 1,
        mem_unit: 1,
        ..Default::default()
    };
    user_safe::write(info_ptr, &info)?;
    Ok(0)
});

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

#[cfg(test)]
mod tests {
    use crate::{
        object::{FileFlags, misc::get_object_current_process},
        process::FdFlags,
        systemcall::{
            implementations::{
                ClockGetres, ClockGettime, ClockNanosleep, ClockSettime, Eventfd, SchedGetaffinity,
                SchedSetaffinity, TimerfdCreate, TimerfdGettime, TimerfdSettime,
            },
            test::{
                TestLinuxItimerspec, TestLinuxTimespec, assert_fd_flags, assert_object_flags,
                close_test_fd, expect_fd,
            },
            test_helpers::{
                SyscallArgs, allocate_user_test_page, assert_user_bytes, expect_errno, expect_ok,
                read_user_value, write_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        clock_getres_syscall,
        "clock_getres accepts null for valid clocks and rejects bad clock ids",
        clock_getres_accepts_null_for_valid_clocks_and_rejects_bad_clock_ids
    );
    crate::test!(
        clock_and_affinity_syscalls,
        "clock and affinity syscalls follow linux pointer rules",
        clock_and_affinity_syscalls_follow_linux_pointer_rules
    );
    crate::test!(
        timerfd_syscalls,
        "timerfd syscalls follow linux flag and timer rules",
        timerfd_syscalls_follow_linux_flag_and_timer_rules
    );

    fn clock_getres_accepts_null_for_valid_clocks_and_rejects_bad_clock_ids() {
        expect_ok(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<ClockGetres>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<ClockGetres>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<ClockGetres>(),
            SyscallError::InvalidArguments,
        );
    }

    fn clock_and_affinity_syscalls_follow_linux_pointer_rules() {
        const CLOCK_REALTIME: u64 = 0;
        const CLOCK_MONOTONIC: u64 = 1;
        const CLOCK_PROCESS_CPUTIME_ID: u64 = 2;
        const CLOCK_THREAD_CPUTIME_ID: u64 = 3;
        const TIMER_ABSTIME: u64 = 1;

        let clock_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([CLOCK_REALTIME, clock_page, 0, 0, 0, 0]).call::<ClockGettime>(),
            0,
        );
        let realtime = read_user_value::<TestLinuxTimespec>(clock_page);
        assert!(realtime.tv_sec >= 0);
        assert!((0..1_000_000_000).contains(&realtime.tv_nsec));
        expect_ok(
            SyscallArgs::new([CLOCK_MONOTONIC, clock_page, 0, 0, 0, 0]).call::<ClockGettime>(),
            0,
        );
        let monotonic = read_user_value::<TestLinuxTimespec>(clock_page);
        assert!(monotonic.tv_sec >= 0);
        assert!((0..1_000_000_000).contains(&monotonic.tv_nsec));
        expect_errno(
            SyscallArgs::new([99, clock_page, 0, 0, 0, 0]).call::<ClockGettime>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([CLOCK_REALTIME, 0, 0, 0, 0, 0]).call::<ClockGettime>(),
            SyscallError::BadAddress,
        );

        write_user_value(
            clock_page,
            &TestLinuxTimespec {
                tv_sec: -1,
                tv_nsec: 0,
            },
        );
        expect_errno(
            SyscallArgs::new([CLOCK_REALTIME, clock_page, 0, 0, 0, 0]).call::<ClockSettime>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(
            clock_page,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
        );
        expect_errno(
            SyscallArgs::new([CLOCK_REALTIME, clock_page, 0, 0, 0, 0]).call::<ClockSettime>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([CLOCK_MONOTONIC, clock_page, 0, 0, 0, 0]).call::<ClockSettime>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([CLOCK_REALTIME, 0, 0, 0, 0, 0]).call::<ClockSettime>(),
            SyscallError::BadAddress,
        );

        write_user_value(
            clock_page,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        );
        expect_ok(
            SyscallArgs::new([CLOCK_MONOTONIC, TIMER_ABSTIME, clock_page, 0, 0, 0])
                .call::<ClockNanosleep>(),
            0,
        );
        write_user_value(
            clock_page,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
        );
        expect_errno(
            SyscallArgs::new([CLOCK_MONOTONIC, 0, clock_page, 0, 0, 0]).call::<ClockNanosleep>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(
            clock_page,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        );
        expect_ok(
            SyscallArgs::new([CLOCK_MONOTONIC, TIMER_ABSTIME, clock_page, 0, 0, 0])
                .call::<ClockNanosleep>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([99, 0, clock_page, 0, 0, 0]).call::<ClockNanosleep>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([CLOCK_PROCESS_CPUTIME_ID, 0, clock_page, 0, 0, 0])
                .call::<ClockNanosleep>(),
            SyscallError::OperationNotSupported,
        );
        expect_errno(
            SyscallArgs::new([CLOCK_THREAD_CPUTIME_ID, 0, clock_page, 0, 0, 0])
                .call::<ClockNanosleep>(),
            SyscallError::OperationNotSupported,
        );
        expect_errno(
            SyscallArgs::new([CLOCK_MONOTONIC, 0, 0, 0, 0, 0]).call::<ClockNanosleep>(),
            SyscallError::BadAddress,
        );

        let mask_page = allocate_user_test_page();
        write_user_value(mask_page, &[1u8, 0, 0, 0, 0, 0, 0, 0]);
        expect_ok(
            SyscallArgs::new([0, 8, mask_page, 0, 0, 0]).call::<SchedSetaffinity>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, 8, mask_page, 0, 0, 0]).call::<SchedSetaffinity>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 0, mask_page, 0, 0, 0]).call::<SchedSetaffinity>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 8, 0, 0, 0, 0]).call::<SchedSetaffinity>(),
            SyscallError::BadAddress,
        );

        expect_ok(
            SyscallArgs::new([0, 8, mask_page, 0, 0, 0]).call::<SchedGetaffinity>(),
            8,
        );
        assert_user_bytes(mask_page, &[1, 0, 0, 0, 0, 0, 0, 0]);
        expect_errno(
            SyscallArgs::new([u64::MAX, 8, mask_page, 0, 0, 0]).call::<SchedGetaffinity>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 4, mask_page, 0, 0, 0]).call::<SchedGetaffinity>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 8, 0, 0, 0, 0]).call::<SchedGetaffinity>(),
            SyscallError::BadAddress,
        );
    }

    fn timerfd_syscalls_follow_linux_flag_and_timer_rules() {
        const TFD_NONBLOCK: u64 = 0o4_000;
        const TFD_CLOEXEC: u64 = 0o2_000_000;
        const CLOCK_REALTIME: u64 = 0;
        const CLOCK_MONOTONIC: u64 = 1;

        let timerfd = expect_fd(
            SyscallArgs::new([CLOCK_MONOTONIC, TFD_NONBLOCK | TFD_CLOEXEC, 0, 0, 0, 0])
                .call::<TimerfdCreate>(),
        );
        assert!(
            get_object_current_process(timerfd as u64)
                .expect("timerfd should resolve")
                .as_timerfd()
                .is_ok()
        );
        assert_fd_flags(timerfd, FdFlags::CLOEXEC);
        assert_object_flags(timerfd, FileFlags::NONBLOCK);
        expect_errno(
            SyscallArgs::new([99, 0, 0, 0, 0, 0]).call::<TimerfdCreate>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([CLOCK_REALTIME, 0x8000_0000, 0, 0, 0, 0]).call::<TimerfdCreate>(),
            SyscallError::InvalidArguments,
        );

        let spec_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([timerfd as u64, spec_page, 0, 0, 0, 0]).call::<TimerfdGettime>(),
            0,
        );
        let spec = read_user_value::<TestLinuxItimerspec>(spec_page);
        assert_eq!(spec.it_interval.tv_sec, 0);
        assert_eq!(spec.it_interval.tv_nsec, 0);
        assert_eq!(spec.it_value.tv_sec, 0);
        assert_eq!(spec.it_value.tv_nsec, 0);
        expect_errno(
            SyscallArgs::new([timerfd as u64, 0, 0, 0, 0, 0]).call::<TimerfdGettime>(),
            SyscallError::BadAddress,
        );

        write_user_value(spec_page, &TestLinuxItimerspec::default());
        expect_ok(
            SyscallArgs::new([timerfd as u64, 0, spec_page, spec_page + 64, 0, 0])
                .call::<TimerfdSettime>(),
            0,
        );
        let old_spec = read_user_value::<TestLinuxItimerspec>(spec_page + 64);
        assert_eq!(old_spec.it_interval.tv_sec, 0);
        assert_eq!(old_spec.it_interval.tv_nsec, 0);
        assert_eq!(old_spec.it_value.tv_sec, 0);
        assert_eq!(old_spec.it_value.tv_nsec, 0);
        expect_errno(
            SyscallArgs::new([timerfd as u64, 0, 0, 0, 0, 0]).call::<TimerfdSettime>(),
            SyscallError::BadAddress,
        );
        write_user_value(
            spec_page,
            &TestLinuxItimerspec {
                it_value: TestLinuxTimespec {
                    tv_sec: 0,
                    tv_nsec: 1_000_000_000,
                },
                ..Default::default()
            },
        );
        expect_errno(
            SyscallArgs::new([timerfd as u64, 0, spec_page, 0, 0, 0]).call::<TimerfdSettime>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, spec_page, 0, 0, 0, 0]).call::<TimerfdGettime>(),
            SyscallError::BadFileDescriptor,
        );
        let non_timerfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        expect_errno(
            SyscallArgs::new([non_timerfd as u64, 0, spec_page, 0, 0, 0]).call::<TimerfdSettime>(),
            SyscallError::BadFileDescriptor,
        );
        close_test_fd(non_timerfd);
        close_test_fd(timerfd);
    }
}
