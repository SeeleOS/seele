use crate::{
    memory::user_safe,
    misc::time::Time,
    misc::timer::{ClockId, Sigevent, TimerNotifyMethod, TimerState},
    process::misc::with_current_process,
    systemcall::{implementations::TimerSetTimeFlags, utils::{SyscallError, SyscallImpl}},
};

use crate::define_syscall;

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
    if timespec.tv_sec < 0 || !(0..1_000_000_000).contains(&timespec.tv_nsec) {
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

define_syscall!(
    TimerCreate,
    |time_type: ClockId, notify_method: *const Sigevent, timer_id: *mut usize| {
        if timer_id.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let notify_method = if notify_method.is_null() {
            TimerNotifyMethod::Signal(crate::signal::Signal::SIGALRM)
        } else {
            TimerNotifyMethod::from(user_safe::read(notify_method)?)
        };

        let id = with_current_process(|process| process.create_timer(time_type, notify_method));
        user_safe::write(timer_id, &id)?;
        Ok(0)
    }
);

define_syscall!(TimerDelete, |id: usize| {
    with_current_process(|process| process.delete_timer(id))?;
    Ok(0)
});

define_syscall!(TimerGetoverrun, |id: usize| {
    with_current_process(|process| process.get_timer_overrun(id))
});

define_syscall!(TimerSettime, |id: usize,
                               flags: TimerSetTimeFlags,
                               timer_state: *const LinuxItimerspec,
                               old_value: *mut LinuxItimerspec| {
    if timer_state.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let new_value = user_safe::read(timer_state)?;
    let value_ns = linux_timespec_to_ns(new_value.it_value)?;
    let interval_ns = linux_timespec_to_ns(new_value.it_interval)?;

    let old_spec = with_current_process(|process| -> Result<LinuxItimerspec, SyscallError> {
        let timer = process
            .timers
            .get_mut(id)
            .ok_or(SyscallError::InvalidArguments)?
            .as_mut()
            .ok_or(SyscallError::InvalidArguments)?;

        let now = match timer.time_type {
            ClockId::Realtime => Time::current(),
            ClockId::SinceBoot => Time::since_boot(),
        };
        let old_spec = match timer.state {
            TimerState::Disabled => LinuxItimerspec::default(),
            TimerState::OneShot { deadline } => LinuxItimerspec {
                it_interval: LinuxTimespec::default(),
                it_value: ns_to_linux_timespec(deadline.sub(now).as_nanoseconds()),
            },
            TimerState::Periodic { deadline, interval } => LinuxItimerspec {
                it_interval: ns_to_linux_timespec(interval.as_nanoseconds()),
                it_value: ns_to_linux_timespec(deadline.sub(now).as_nanoseconds()),
            },
        };

        timer.state = if value_ns == 0 {
            TimerState::Disabled
        } else {
            let now = match timer.time_type {
                ClockId::Realtime => Time::current(),
                ClockId::SinceBoot => Time::since_boot(),
            };
            let deadline = if flags.contains(TimerSetTimeFlags::TFD_TIMER_ABSTIME) {
                Time::from_nanoseconds(value_ns)
            } else {
                now.add_ns(value_ns)
            };

            if interval_ns == 0 {
                TimerState::OneShot { deadline }
            } else {
                TimerState::Periodic {
                    deadline,
                    interval: Time::from_nanoseconds(interval_ns),
                }
            }
        };

        Ok(old_spec)
    })?;

    if !old_value.is_null() {
        user_safe::write(old_value, &old_spec)?;
    }
    Ok(0)
});

define_syscall!(TimerGettime, |id: usize, timer_state: *mut LinuxItimerspec| {
    if timer_state.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let spec = with_current_process(|process| -> Result<LinuxItimerspec, SyscallError> {
        let timer = process
            .timers
            .get_mut(id)
            .ok_or(SyscallError::InvalidArguments)?
            .as_mut()
            .ok_or(SyscallError::InvalidArguments)?;
        let now = match timer.time_type {
            ClockId::Realtime => Time::current(),
            ClockId::SinceBoot => Time::since_boot(),
        };
        let spec = match timer.state {
            TimerState::Disabled => LinuxItimerspec::default(),
            TimerState::OneShot { deadline } => LinuxItimerspec {
                it_interval: LinuxTimespec::default(),
                it_value: ns_to_linux_timespec(deadline.sub(now).as_nanoseconds()),
            },
            TimerState::Periodic { deadline, interval } => LinuxItimerspec {
                it_interval: ns_to_linux_timespec(interval.as_nanoseconds()),
                it_value: ns_to_linux_timespec(deadline.sub(now).as_nanoseconds()),
            },
        };
        Ok(spec)
    })?;
    user_safe::write(timer_state, &spec)?;
    Ok(0)
});
