use crate::{
    misc::{
        signal::{PendingSignalInfo, SI_QUEUE, SigInfo, Signal, Signals},
        time::Time,
        timer::{Sigevent, TimerMode, TimerNotify, TimerNotifyMethod, TimerSpec, TimerState},
    },
    process::ProcessExitStatus,
};

crate::test!(
    time_arithmetic,
    "time arithmetic saturates and splits subseconds",
    time_arithmetic_saturates_and_splits_subseconds
);
crate::test!(
    timer_state_conversion,
    "timer spec round trips state and notify method",
    timer_spec_round_trips_state_and_notify_method
);
crate::test!(
    signal_siginfo_conversion,
    "signal masks and siginfo conversion match linux numbers",
    signal_masks_and_siginfo_conversion_match_linux_numbers
);
crate::test!(
    process_exit_wait_encodings,
    "process exit status exports wait encodings",
    process_exit_status_exports_wait_encodings
);

fn time_arithmetic_saturates_and_splits_subseconds() {
    let time = Time::from_nanoseconds(1_234_567_890);

    assert_eq!(time.as_seconds(), 1);
    assert_eq!(time.as_milliseconds(), 1234);
    assert_eq!(time.as_microseconds(), 1_234_567);
    assert_eq!(time.subsec_nanoseconds(), 234_567_890);
    assert_eq!(
        Time::from_nanoseconds(5)
            .sub(Time::from_nanoseconds(9))
            .as_nanoseconds(),
        0
    );
    assert_eq!(
        Time::from_nanoseconds(u64::MAX).add_ns(1).as_nanoseconds(),
        u64::MAX
    );
}

fn timer_spec_round_trips_state_and_notify_method() {
    let spec = TimerSpec {
        state_type: TimerMode::Periodic,
        deadline: 10,
        interval: 3,
    };
    let state = TimerState::from(spec);
    assert!(matches!(
        state,
        TimerState::Periodic {
            deadline: Time(10),
            interval: Time(3)
        }
    ));
    assert_eq!(TimerSpec::from(state).deadline, 10);

    assert!(matches!(
        TimerNotifyMethod::from(Sigevent {
            notify_type: TimerNotify::Signal,
            signal: Signal::SIGUSR1
        }),
        TimerNotifyMethod::Signal(Signal::SIGUSR1)
    ));
}

fn signal_masks_and_siginfo_conversion_match_linux_numbers() {
    assert_eq!(Signal::SIGINT.index(), 1);
    assert_eq!(Signal::SIGINT.mask(), Signals::SIGINT.bits());
    assert!(Signal::SIGKILL.is_unblockable());
    assert!(Signal::SIGRTMIN.is_realtime());

    let info = SigInfo::for_process_signal(Signal::SIGTERM, 123, 1000);
    let pending = PendingSignalInfo::from_siginfo(info);
    assert_eq!(pending.si_signo, Signal::SIGTERM as i32);
    assert_eq!(pending.si_pid, 123);
    assert_eq!(pending.si_uid, 1000);

    let queued = PendingSignalInfo {
        si_signo: Signal::SIGUSR1 as i32,
        si_code: SI_QUEUE,
        si_pid: 77,
        si_uid: 88,
        si_value: 0xfeed,
        ..Default::default()
    }
    .to_siginfo();
    assert_eq!(queued.si_signo, Signal::SIGUSR1 as i32);
    assert_eq!(queued.si_code, SI_QUEUE);
    assert_eq!(queued.sender_pid(), 77);
    assert_eq!(queued.sender_uid(), 88);
    assert_eq!(queued.signal_value_ptr(), 0xfeed);
}

fn process_exit_status_exports_wait_encodings() {
    assert_eq!(
        ProcessExitStatus::from_exit_code(0x123).wait_status(),
        0x2300
    );
    assert_eq!(ProcessExitStatus::Exited(7).waitid_status(), 7);
    assert_eq!(
        ProcessExitStatus::Signaled(Signal::SIGKILL).wait_status(),
        Signal::SIGKILL as i32
    );
}
