use crate::{
    memory::user_safe,
    misc::time::Time,
    misc::timer::{ClockId, Sigevent, TimerNotifyMethod, TimerState},
    process::misc::with_current_process,
    systemcall::{
        implementations::TimerSetTimeFlags,
        utils::{SyscallError, SyscallImpl},
    },
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

#[cfg(test)]
mod tests {
    use crate::{
        signal::Signal,
        systemcall::{
            implementations::{
                TimerCreate, TimerDelete, TimerGetoverrun, TimerGettime, TimerSettime,
            },
            test::{TestLinuxItimerspec, TestLinuxTimespec, allocate_large_user_test_region},
            test_helpers::{
                SyscallArgs, assert_linux_layout, expect_errno, expect_ok, read_user_value,
                write_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        posix_timer_syscalls,
        "posix timer syscalls follow linux rules",
        posix_timer_syscalls_follow_linux_rules
    );

    fn posix_timer_syscalls_follow_linux_rules() {
        const CLOCK_REALTIME: u64 = 0;
        const TIMER_ABSTIME: u64 = 1;
        const SIGEV_NONE: u8 = 0;
        const SIGEV_SIGNAL: u8 = 1;

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct TestLinuxSigevent {
            notify_type: u8,
            signal: Signal,
        }

        assert_linux_layout::<TestLinuxItimerspec>(32, 8);

        let page = allocate_large_user_test_region(4);
        write_user_value(
            page,
            &TestLinuxSigevent {
                notify_type: SIGEV_SIGNAL,
                signal: Signal::SIGUSR1,
            },
        );

        expect_errno(
            SyscallArgs::new([CLOCK_REALTIME, page, 0, 0, 0, 0]).call::<TimerCreate>(),
            SyscallError::BadAddress,
        );

        expect_errno(
            SyscallArgs::new([99, page, page + 64, 0, 0, 0]).call::<TimerCreate>(),
            SyscallError::InvalidArguments,
        );

        let timer_id_page = page + 64;
        expect_ok(
            SyscallArgs::new([CLOCK_REALTIME, page, timer_id_page, 0, 0, 0]).call::<TimerCreate>(),
            0,
        );
        let signal_timer_id = read_user_value::<usize>(timer_id_page);

        expect_ok(
            SyscallArgs::new([CLOCK_REALTIME, 0, timer_id_page + 8, 0, 0, 0]).call::<TimerCreate>(),
            0,
        );
        let default_timer_id = read_user_value::<usize>(timer_id_page + 8);
        assert_ne!(signal_timer_id, default_timer_id);

        expect_ok(
            SyscallArgs::new([signal_timer_id as u64, page + 128, 0, 0, 0, 0])
                .call::<TimerGettime>(),
            0,
        );
        let initial = read_user_value::<TestLinuxItimerspec>(page + 128);
        assert_eq!(initial.it_value.tv_sec, 0);
        assert_eq!(initial.it_value.tv_nsec, 0);
        assert_eq!(initial.it_interval.tv_sec, 0);
        assert_eq!(initial.it_interval.tv_nsec, 0);

        write_user_value(
            page + 192,
            &TestLinuxItimerspec {
                it_interval: TestLinuxTimespec {
                    tv_sec: 2,
                    tv_nsec: 3,
                },
                it_value: TestLinuxTimespec {
                    tv_sec: 4,
                    tv_nsec: 5,
                },
            },
        );
        expect_ok(
            SyscallArgs::new([signal_timer_id as u64, 0, page + 192, page + 256, 0, 0])
                .call::<TimerSettime>(),
            0,
        );
        let old_spec = read_user_value::<TestLinuxItimerspec>(page + 256);
        assert_eq!(old_spec.it_value.tv_sec, 0);
        assert_eq!(old_spec.it_value.tv_nsec, 0);
        assert_eq!(old_spec.it_interval.tv_sec, 0);
        assert_eq!(old_spec.it_interval.tv_nsec, 0);

        expect_ok(
            SyscallArgs::new([signal_timer_id as u64, page + 320, 0, 0, 0, 0])
                .call::<TimerGettime>(),
            0,
        );
        let armed = read_user_value::<TestLinuxItimerspec>(page + 320);
        assert_eq!(armed.it_interval.tv_sec, 2);
        assert_eq!(armed.it_interval.tv_nsec, 3);
        assert!(armed.it_value.tv_sec <= 4);
        assert!(armed.it_value.tv_nsec < 1_000_000_000);

        write_user_value(
            page + 384,
            &TestLinuxItimerspec {
                it_interval: TestLinuxTimespec::default(),
                it_value: TestLinuxTimespec::default(),
            },
        );
        expect_ok(
            SyscallArgs::new([signal_timer_id as u64, TIMER_ABSTIME, page + 384, 0, 0, 0])
                .call::<TimerSettime>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([signal_timer_id as u64, page + 448, 0, 0, 0, 0])
                .call::<TimerGettime>(),
            0,
        );
        let disarmed = read_user_value::<TestLinuxItimerspec>(page + 448);
        assert_eq!(disarmed.it_value.tv_sec, 0);
        assert_eq!(disarmed.it_value.tv_nsec, 0);
        assert_eq!(disarmed.it_interval.tv_sec, 0);
        assert_eq!(disarmed.it_interval.tv_nsec, 0);

        expect_ok(
            SyscallArgs::new([signal_timer_id as u64, 0, 0, 0, 0, 0]).call::<TimerGetoverrun>(),
            0,
        );

        expect_errno(
            SyscallArgs::new([signal_timer_id as u64, 0, 0, 0, 0, 0]).call::<TimerSettime>(),
            SyscallError::BadAddress,
        );
        write_user_value(
            page + 512,
            &TestLinuxItimerspec {
                it_value: TestLinuxTimespec {
                    tv_sec: 0,
                    tv_nsec: 1_000_000_000,
                },
                ..Default::default()
            },
        );
        expect_errno(
            SyscallArgs::new([signal_timer_id as u64, 0, page + 512, 0, 0, 0])
                .call::<TimerSettime>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([signal_timer_id as u64, 0, 0, 0, 0, 0]).call::<TimerGettime>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([signal_timer_id as u64, 0, 1, 0, 0, 0]).call::<TimerSettime>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([usize::MAX as u64, 0, 0, 0, 0, 0]).call::<TimerDelete>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([usize::MAX as u64, 0, 0, 0, 0, 0]).call::<TimerGetoverrun>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([default_timer_id as u64, 0, 0, 0, 0, 0]).call::<TimerDelete>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([default_timer_id as u64, page + 640, 0, 0, 0, 0])
                .call::<TimerGettime>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([signal_timer_id as u64, 0, 0, 0, 0, 0]).call::<TimerDelete>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([signal_timer_id as u64, 0, 0, 0, 0, 0]).call::<TimerGetoverrun>(),
            SyscallError::InvalidArguments,
        );

        let _ = SIGEV_NONE;
    }
}

define_syscall!(TimerDelete, |id: usize| {
    with_current_process(|process| process.delete_timer(id))?;
    Ok(0)
});

define_syscall!(TimerGetoverrun, |id: usize| {
    with_current_process(|process| process.get_timer_overrun(id))
});

define_syscall!(
    TimerSettime,
    |id: usize,
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
    }
);

define_syscall!(
    TimerGettime,
    |id: usize, timer_state: *mut LinuxItimerspec| {
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
    }
);
