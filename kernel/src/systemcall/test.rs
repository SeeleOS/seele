use crate::{
    memory::protection::Protection,
    misc::timer::ClockId,
    signal::{Signal, Signals},
    systemcall::{
        arg_types::SyscallArg,
        implementations::{
            OpenFlags, PollEvents, PollTimespec, SelectTimespec, clear_fdset, fdset_contains,
            fdset_insert, fdset_words, kernel_events_for, saturating_timeout_ms, timeout_is_zero,
            timeout_to_deadline, translate_ready_events,
        },
        linux_semantics::{
            KNOWN_LINUX_SYSCALL_COVERAGE_GAPS, LINUX_SYSCALL_SEMANTICS_COVERAGE,
            LinuxSyscallTestKind,
        },
        numbers::SyscallNumber,
        table::{REGISTERED_SYSCALLS, SYSCALL_TABLE},
        test_helpers::{SyscallArgs, assert_linux_layout, errno_code, expect_errno, expect_ok},
        utils::SyscallError,
    },
};

crate::test!(
    syscall_number_lookup,
    "syscall number lookup matches x86_64 abi values",
    syscall_number_lookup_matches_x86_64_abi_values
);
crate::test!(
    syscall_table_coverage,
    "syscall table contains registered and rejects unknown numbers",
    syscall_table_contains_registered_and_rejects_unknown_numbers
);
crate::test!(
    syscall_registration_list,
    "registered syscall list matches populated table slots",
    registered_syscall_list_matches_populated_table_slots
);
crate::test!(
    linux_syscall_semantics_coverage_ledger,
    "linux syscall semantics ledger covers every registered syscall exactly once",
    linux_syscall_semantics_ledger_covers_every_registered_syscall_exactly_once
);
crate::test!(
    syscall_test_helpers,
    "syscall test helpers assert linux errno return and layout expectations",
    syscall_test_helpers_assert_linux_errno_return_and_layout_expectations
);
crate::test!(
    typed_syscall_arg_conversion,
    "typed syscall args convert flags and enums at boundary",
    typed_syscall_args_convert_flags_and_enums_at_boundary
);
crate::test!(
    poll_event_translation,
    "poll helpers translate linux events to kernel readiness",
    poll_helpers_translate_linux_events_to_kernel_readiness
);
crate::test!(
    poll_timeout_validation,
    "poll timeout helpers reject invalid timespecs and saturate",
    poll_timeout_helpers_reject_invalid_timespecs_and_saturate
);
crate::test!(
    select_fdset_helpers,
    "select fdset helpers count clear test and set words",
    select_fdset_helpers_count_clear_test_and_set_words
);
crate::test!(
    select_timeout_validation,
    "select timeout helpers validate null zero and invalid timespecs",
    select_timeout_helpers_validate_null_zero_and_invalid_timespecs
);

fn syscall_number_lookup_matches_x86_64_abi_values() {
    assert_eq!(SyscallNumber::from_number(0), Some(SyscallNumber::Read));
    assert_eq!(SyscallNumber::from_number(1), Some(SyscallNumber::Write));
    assert_eq!(SyscallNumber::from_number(257), Some(SyscallNumber::OpenAt));
    assert_eq!(SyscallNumber::from_number(999), None);
}

fn syscall_table_contains_registered_and_rejects_unknown_numbers() {
    assert!(SYSCALL_TABLE[SyscallNumber::Read as usize].is_some());
    assert!(SYSCALL_TABLE[SyscallNumber::OpenAt as usize].is_some());
    assert!(SYSCALL_TABLE[999].is_none());
}

fn registered_syscall_list_matches_populated_table_slots() {
    for &number in REGISTERED_SYSCALLS {
        assert!(
            SYSCALL_TABLE[number as usize].is_some(),
            "registered syscall {number:?} is missing from SYSCALL_TABLE"
        );
    }

    for (index, handler) in SYSCALL_TABLE.iter().enumerate() {
        if handler.is_none() {
            continue;
        }

        let Some(number) = SyscallNumber::from_number(index) else {
            panic!("SYSCALL_TABLE contains handler at unknown syscall number {index}");
        };

        assert!(
            REGISTERED_SYSCALLS.contains(&number),
            "SYSCALL_TABLE contains {number:?} but REGISTERED_SYSCALLS does not"
        );
    }
}

fn linux_syscall_semantics_ledger_covers_every_registered_syscall_exactly_once() {
    let mut covered = [false; 1500];
    let mut coverage_gaps = 0;

    for entry in LINUX_SYSCALL_SEMANTICS_COVERAGE {
        let index = entry.number as usize;
        assert!(
            !covered[index],
            "duplicate semantics ledger entry for {:?}",
            entry.number
        );
        covered[index] = true;
        assert!(
            REGISTERED_SYSCALLS.contains(&entry.number),
            "semantics ledger contains unregistered syscall {:?}",
            entry.number
        );
        assert!(
            !entry.test.is_empty(),
            "semantics ledger entry {:?} must describe its test coverage",
            entry.number
        );

        if entry.kind == LinuxSyscallTestKind::CoverageGap {
            coverage_gaps += 1;
        }
    }

    for &number in REGISTERED_SYSCALLS {
        assert!(
            covered[number as usize],
            "registered syscall {number:?} has no Linux semantics ledger entry"
        );
    }

    assert!(
        coverage_gaps <= KNOWN_LINUX_SYSCALL_COVERAGE_GAPS,
        "Linux syscall semantics CoverageGap entries increased from {} to {}; new registered syscalls need Unit or Integration behavior tests",
        KNOWN_LINUX_SYSCALL_COVERAGE_GAPS,
        coverage_gaps
    );
}

fn syscall_test_helpers_assert_linux_errno_return_and_layout_expectations() {
    assert_eq!(SyscallArgs::none().0, [0; 6]);
    assert_eq!(SyscallArgs::new([1, 2, 3, 4, 5, 6]).0[5], 6);
    expect_ok(Ok(0), 0);
    expect_errno(
        Err(SyscallError::InvalidArguments),
        SyscallError::InvalidArguments,
    );
    assert_eq!(errno_code(SyscallError::BadAddress), -14);
    assert_linux_layout::<u64>(8, 8);
}

fn typed_syscall_args_convert_flags_and_enums_at_boundary() {
    assert_eq!(<u32 as SyscallArg>::from_u64(u64::MAX).unwrap(), u32::MAX);
    assert!(<bool as SyscallArg>::from_u64(2).unwrap());
    assert_eq!(
        <Signal as SyscallArg>::from_u64(Signal::SIGTERM as u64).unwrap(),
        Signal::SIGTERM
    );
    assert!(matches!(
        <Signal as SyscallArg>::from_u64(0),
        Err(SyscallError::InvalidArguments)
    ));
    assert_eq!(
        <ClockId as SyscallArg>::from_u64(ClockId::Realtime as u64).unwrap(),
        ClockId::Realtime
    );
    assert_eq!(
        <Protection as SyscallArg>::from_u64((Protection::READ | Protection::WRITE).bits())
            .unwrap()
            .bits(),
        (Protection::READ | Protection::WRITE).bits()
    );
    assert_eq!(
        <Signals as SyscallArg>::from_u64(Signal::SIGINT.mask())
            .unwrap()
            .bits(),
        Signals::SIGINT.bits()
    );
    assert_eq!(
        <OpenFlags as SyscallArg>::from_u64(
            (OpenFlags::CLOEXEC | OpenFlags::NONBLOCK).bits() as u64
        )
        .unwrap()
        .bits(),
        (OpenFlags::CLOEXEC | OpenFlags::NONBLOCK).bits()
    );
    assert!(<PollEvents as SyscallArg>::from_u64(PollEvents::POLLIN.bits() as u64).is_ok());
}

fn poll_helpers_translate_linux_events_to_kernel_readiness() {
    let events = kernel_events_for(PollEvents::POLLIN | PollEvents::POLLOUT);

    assert_eq!(
        events[0],
        Some(crate::polling::event::PollableEvent::CanBeRead)
    );
    assert_eq!(
        events[1],
        Some(crate::polling::event::PollableEvent::CanBeWritten)
    );
    assert_eq!(events[2], Some(crate::polling::event::PollableEvent::Error));
    assert_eq!(
        events[3],
        Some(crate::polling::event::PollableEvent::Closed)
    );

    let translated = translate_ready_events(
        PollEvents::POLLIN | PollEvents::POLLRDNORM | PollEvents::POLLHUP,
        (PollEvents::POLLIN | PollEvents::POLLHUP).bits() as u32,
    );
    let translated = PollEvents::from_bits_retain(translated);
    assert!(translated.contains(PollEvents::POLLIN));
    assert!(translated.contains(PollEvents::POLLRDNORM));
    assert!(translated.contains(PollEvents::POLLHUP));
    assert!(!translated.contains(PollEvents::POLLOUT));
}

fn poll_timeout_helpers_reject_invalid_timespecs_and_saturate() {
    assert_eq!(
        saturating_timeout_ms(&PollTimespec {
            tv_sec: 1,
            tv_nsec: 999_999_999,
        })
        .unwrap(),
        1999
    );
    assert_eq!(
        saturating_timeout_ms(&PollTimespec {
            tv_sec: i64::MAX,
            tv_nsec: 0,
        })
        .unwrap(),
        i32::MAX
    );
    assert!(matches!(
        saturating_timeout_ms(&PollTimespec {
            tv_sec: -1,
            tv_nsec: 0,
        }),
        Err(SyscallError::InvalidArguments)
    ));
    assert!(matches!(
        saturating_timeout_ms(&PollTimespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        }),
        Err(SyscallError::InvalidArguments)
    ));
}

fn select_fdset_helpers_count_clear_test_and_set_words() {
    assert_eq!(fdset_words(0), 0);
    assert_eq!(fdset_words(1), 1);
    assert_eq!(fdset_words(65), 2);

    let mut words = [0u64; 2];
    unsafe {
        fdset_insert(words.as_mut_ptr(), 0);
        fdset_insert(words.as_mut_ptr(), 64);
        assert!(fdset_contains(words.as_ptr(), 0));
        assert!(fdset_contains(words.as_ptr(), 64));
        assert!(!fdset_contains(words.as_ptr(), 63));
        clear_fdset(words.as_mut_ptr(), 65);
    }

    assert_eq!(words, [0, 0]);
}

fn select_timeout_helpers_validate_null_zero_and_invalid_timespecs() {
    assert!(timeout_to_deadline(core::ptr::null()).unwrap().is_none());
    assert!(!timeout_is_zero(core::ptr::null()));

    let zero = SelectTimespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    assert!(timeout_is_zero(&zero));
    assert!(timeout_to_deadline(&zero).unwrap().is_some());

    let invalid = SelectTimespec {
        tv_sec: 0,
        tv_nsec: 1_000_000_000,
    };
    assert!(matches!(
        timeout_to_deadline(&invalid),
        Err(SyscallError::InvalidArguments)
    ));
}
