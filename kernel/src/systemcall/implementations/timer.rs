use crate::{
    memory::user_safe,
    misc::time::Time,
    misc::timer::{ClockId, TimerNotifyMethod, TimerState, process_expired_process_timers},
    process::misc::with_current_process,
    signal::Signal,
    systemcall::{
        implementations::TimerSetTimeFlags,
        utils::{SyscallError, SyscallImpl},
    },
    thread::scheduling::request_all_cpus_resched,
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

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxSigevent {
    sigev_value: u64,
    sigev_signo: i32,
    sigev_notify: i32,
}

impl TryFrom<LinuxSigevent> for TimerNotifyMethod {
    type Error = SyscallError;

    fn try_from(value: LinuxSigevent) -> Result<Self, Self::Error> {
        const SIGEV_SIGNAL: i32 = 0;
        const SIGEV_NONE: i32 = 1;
        const SIGEV_THREAD: i32 = 2;
        const SIGEV_THREAD_ID: i32 = 4;

        match value.sigev_notify {
            SIGEV_NONE => Ok(Self::None),
            SIGEV_SIGNAL | SIGEV_THREAD | SIGEV_THREAD_ID => {
                let signal = Signal::try_from(value.sigev_signo as u64)
                    .map_err(|_| SyscallError::InvalidArguments)?;
                Ok(Self::Signal(signal))
            }
            _ => Err(SyscallError::InvalidArguments),
        }
    }
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

fn timer_id_to_index(id: i32) -> Result<usize, SyscallError> {
    usize::try_from(id).map_err(|_| SyscallError::InvalidArguments)
}

define_syscall!(
    TimerCreate,
    |time_type: ClockId, notify_method: *const LinuxSigevent, timer_id: *mut i32| {
        if timer_id.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let notify_method = if notify_method.is_null() {
            TimerNotifyMethod::Signal(crate::signal::Signal::SIGALRM)
        } else {
            TimerNotifyMethod::try_from(user_safe::read(notify_method)?)?
        };

        let id = with_current_process(|process| process.create_timer(time_type, notify_method));
        let id = i32::try_from(id).map_err(|_| SyscallError::InvalidArguments)?;
        user_safe::write(timer_id, &id)?;
        Ok(0)
    }
);

#[cfg(test)]
mod tests {
    use crate::{
        misc::time::Time,
        process::manager::get_current_process,
        signal::{Signal, Signals},
        systemcall::{
            implementations::{
                ClockSettime, TimerCreate, TimerDelete, TimerGetoverrun, TimerGettime, TimerSettime,
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
        const SIGEV_SIGNAL: i32 = 0;
        const SIGEV_NONE: i32 = 1;

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct TestLinuxSigevent {
            value: u64,
            signal: i32,
            notify_type: i32,
        }

        assert_linux_layout::<TestLinuxItimerspec>(32, 8);

        let page = allocate_large_user_test_region(4);
        write_user_value(
            page,
            &TestLinuxSigevent {
                value: 0,
                signal: Signal::SIGUSR1 as i32,
                notify_type: SIGEV_SIGNAL,
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
        let signal_timer_id = read_user_value::<i32>(timer_id_page);

        expect_ok(
            SyscallArgs::new([CLOCK_REALTIME, 0, timer_id_page + 4, 0, 0, 0]).call::<TimerCreate>(),
            0,
        );
        let default_timer_id = read_user_value::<i32>(timer_id_page + 4);
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
            SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<TimerDelete>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<TimerGetoverrun>(),
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

    crate::test!(
        realtime_absolute_posix_timer,
        "realtime absolute posix timer follows wallclock jumps",
        realtime_absolute_posix_timer_follows_wallclock_jumps
    );

    fn realtime_absolute_posix_timer_follows_wallclock_jumps() {
        const CLOCK_REALTIME: u64 = 0;
        const TIMER_ABSTIME: u64 = 1;
        const SIGEV_SIGNAL: i32 = 0;

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct TestLinuxSigevent {
            value: u64,
            signal: i32,
            notify_type: i32,
        }

        let page = allocate_large_user_test_region(2);
        let process = get_current_process();
        let old_timers = core::mem::take(&mut process.lock().timers);
        let old_realtime = Time::current();
        let old_pending_signals = process.lock().pending_signals;

        write_user_value(
            page,
            &TestLinuxSigevent {
                value: 0,
                signal: Signal::SIGABRT as i32,
                notify_type: SIGEV_SIGNAL,
            },
        );
        expect_ok(
            SyscallArgs::new([CLOCK_REALTIME, page, page + 64, 0, 0, 0]).call::<TimerCreate>(),
            0,
        );
        let timer_id = read_user_value::<i32>(page + 64);

        write_user_value(
            page + 128,
            &TestLinuxTimespec {
                tv_sec: 0x7fff_fffe,
                tv_nsec: 0,
            },
        );
        expect_ok(
            SyscallArgs::new([CLOCK_REALTIME, page + 128, 0, 0, 0, 0]).call::<ClockSettime>(),
            0,
        );
        write_user_value(
            page + 192,
            &TestLinuxItimerspec {
                it_interval: TestLinuxTimespec::default(),
                it_value: TestLinuxTimespec {
                    tv_sec: 0x8000_0001,
                    tv_nsec: 0,
                },
            },
        );
        expect_ok(
            SyscallArgs::new([timer_id as u64, TIMER_ABSTIME, page + 192, 0, 0, 0])
                .call::<TimerSettime>(),
            0,
        );

        {
            let mut process = process.lock();
            let deadline = process
                .next_timer_deadline()
                .expect("armed realtime timer should expose a scheduler deadline");
            assert!(deadline > Time::since_boot());
            assert!(deadline <= Time::since_boot().add_ns(4_000_000_000));
            assert!(
                !process
                    .pending_signals
                    .contains(Signals::from(Signal::SIGABRT))
            );
        }

        write_user_value(
            page + 256,
            &TestLinuxTimespec {
                tv_sec: 0x8000_0001,
                tv_nsec: 0,
            },
        );
        expect_ok(
            SyscallArgs::new([CLOCK_REALTIME, page + 256, 0, 0, 0, 0]).call::<ClockSettime>(),
            0,
        );
        assert!(
            process
                .lock()
                .pending_signals
                .contains(Signals::from(Signal::SIGABRT))
        );

        {
            let mut process = process.lock();
            process.timers = old_timers;
            process.pending_signals = old_pending_signals;
        }
        crate::misc::time::set_unix_timestamp_nanoseconds(old_realtime.as_nanoseconds() as i64);
    }
}

define_syscall!(TimerDelete, |id: i32| {
    let id = timer_id_to_index(id)?;
    with_current_process(|process| process.delete_timer(id))?;
    Ok(0)
});

define_syscall!(TimerGetoverrun, |id: i32| {
    let id = timer_id_to_index(id)?;
    with_current_process(|process| process.get_timer_overrun(id))
});

define_syscall!(
    TimerSettime,
    |id: i32,
     flags: TimerSetTimeFlags,
     timer_state: *const LinuxItimerspec,
     old_value: *mut LinuxItimerspec| {
        if timer_state.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let id = timer_id_to_index(id)?;
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
                ClockId::SinceBoot | ClockId::ProcessCpu | ClockId::ThreadCpu => Time::since_boot(),
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
                    ClockId::SinceBoot | ClockId::ProcessCpu | ClockId::ThreadCpu => {
                        Time::since_boot()
                    }
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
        process_expired_process_timers();
        request_all_cpus_resched();
        Ok(0)
    }
);

define_syscall!(
    TimerGettime,
    |id: i32, timer_state: *mut LinuxItimerspec| {
        if timer_state.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let id = timer_id_to_index(id)?;
        let spec = with_current_process(|process| -> Result<LinuxItimerspec, SyscallError> {
            let timer = process
                .timers
                .get_mut(id)
                .ok_or(SyscallError::InvalidArguments)?
                .as_mut()
                .ok_or(SyscallError::InvalidArguments)?;
            let now = match timer.time_type {
                ClockId::Realtime => Time::current(),
                ClockId::SinceBoot | ClockId::ProcessCpu | ClockId::ThreadCpu => Time::since_boot(),
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
