use crate::{
    memory::protection::Protection,
    misc::timer::ClockId,
    process::{Process, group::ProcessGroupID, manager::get_current_process},
    signal::{Signal, Signals},
    systemcall::{
        arg_types::SyscallArg,
        implementations::{
            ClockGetres, Getegid, Geteuid, Getgid, Getgroups, Getpgid, Getpgrp, Getpid, Getppid,
            Getuid, OpenFlags, PollEvents, PollTimespec, SelectTimespec, Setfsgid, Setfsuid,
            Setgid, Setgroups, Setpgid, Setregid, Setresgid, Setresuid, Setreuid, Setuid,
            clear_fdset, fdset_contains, fdset_insert, fdset_words, kernel_events_for,
            saturating_timeout_ms, timeout_is_zero, timeout_to_deadline, translate_ready_events,
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
    process_identity_syscalls,
    "process identity syscalls match current linux task state",
    process_identity_syscalls_match_current_linux_task_state
);
crate::test!(
    process_group_syscalls,
    "process group syscalls follow linux pid zero and esrch rules",
    process_group_syscalls_follow_linux_pid_zero_and_esrch_rules
);
crate::test!(
    credential_getter_syscalls,
    "credential getters return current linux ids",
    credential_getters_return_current_linux_ids
);
crate::test!(
    credential_setter_syscalls,
    "credential setters update linux real effective saved and fs ids",
    credential_setters_update_linux_real_effective_saved_and_fs_ids
);
crate::test!(
    fsuid_fsgid_syscalls,
    "fsuid and fsgid syscalls return previous ids and update state",
    fsuid_fsgid_syscalls_return_previous_ids_and_update_state
);
crate::test!(
    group_syscalls,
    "group syscalls validate linux size rules",
    group_syscalls_validate_linux_size_rules
);
crate::test!(
    clock_getres_syscall,
    "clock_getres accepts null for valid clocks and rejects bad clock ids",
    clock_getres_accepts_null_for_valid_clocks_and_rejects_bad_clock_ids
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

struct CredentialSnapshot {
    real_uid: u32,
    effective_uid: u32,
    saved_uid: u32,
    fs_uid: u32,
    real_gid: u32,
    effective_gid: u32,
    saved_gid: u32,
    fs_gid: u32,
}

impl CredentialSnapshot {
    fn save(process: &Process) -> Self {
        Self {
            real_uid: process.real_uid,
            effective_uid: process.effective_uid,
            saved_uid: process.saved_uid,
            fs_uid: process.fs_uid,
            real_gid: process.real_gid,
            effective_gid: process.effective_gid,
            saved_gid: process.saved_gid,
            fs_gid: process.fs_gid,
        }
    }

    fn save_current() -> Self {
        let process = get_current_process();
        let process = process.lock();
        Self::save(&process)
    }

    fn restore(self) {
        let process = get_current_process();
        let mut process = process.lock();
        process.real_uid = self.real_uid;
        process.effective_uid = self.effective_uid;
        process.saved_uid = self.saved_uid;
        process.fs_uid = self.fs_uid;
        process.real_gid = self.real_gid;
        process.effective_gid = self.effective_gid;
        process.saved_gid = self.saved_gid;
        process.fs_gid = self.fs_gid;
    }
}

fn process_identity_syscalls_match_current_linux_task_state() {
    let (pid, ppid, group_id) = {
        let process = get_current_process();
        let process = process.lock();
        (
            process.pid.0 as usize,
            process
                .parent
                .as_ref()
                .map(|parent| parent.lock().pid.0 as usize)
                .unwrap_or(0),
            process.group_id.0 as usize,
        )
    };

    expect_ok(SyscallArgs::none().call::<Getpid>(), pid);
    expect_ok(SyscallArgs::none().call::<Getppid>(), ppid);
    expect_ok(SyscallArgs::none().call::<Getpgrp>(), group_id);
}

fn process_group_syscalls_follow_linux_pid_zero_and_esrch_rules() {
    let process = get_current_process();
    let old_group = {
        let process = process.lock();
        process.group_id
    };

    expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Setpgid>(), 0);
    {
        let process = process.lock();
        assert_eq!(process.group_id, ProcessGroupID::from_leader(process.pid));
    }
    expect_ok(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getpgid>(),
        get_current_process().lock().group_id.0 as usize,
    );
    expect_errno(
        SyscallArgs::new([u64::from(u32::MAX), 0, 0, 0, 0, 0]).call::<Getpgid>(),
        SyscallError::NoProcess,
    );

    {
        let mut process = process.lock();
        process.group_id = old_group;
    }
}

fn credential_getters_return_current_linux_ids() {
    let process = get_current_process();
    let mut process = process.lock();
    let saved = CredentialSnapshot::save(&process);
    process.real_uid = 1001;
    process.effective_uid = 1002;
    process.real_gid = 1003;
    process.effective_gid = 1004;
    drop(process);

    expect_ok(SyscallArgs::none().call::<Getuid>(), 1001);
    expect_ok(SyscallArgs::none().call::<Geteuid>(), 1002);
    expect_ok(SyscallArgs::none().call::<Getgid>(), 1003);
    expect_ok(SyscallArgs::none().call::<Getegid>(), 1004);

    saved.restore();
}

fn credential_setters_update_linux_real_effective_saved_and_fs_ids() {
    let saved = CredentialSnapshot::save_current();

    expect_ok(SyscallArgs::new([42, 0, 0, 0, 0, 0]).call::<Setuid>(), 0);
    {
        let process = get_current_process();
        let process = process.lock();
        assert_eq!(process.real_uid, 42);
        assert_eq!(process.effective_uid, 42);
        assert_eq!(process.saved_uid, 42);
        assert_eq!(process.fs_uid, 42);
    }

    expect_ok(SyscallArgs::new([43, 0, 0, 0, 0, 0]).call::<Setgid>(), 0);
    {
        let process = get_current_process();
        let process = process.lock();
        assert_eq!(process.real_gid, 43);
        assert_eq!(process.effective_gid, 43);
        assert_eq!(process.saved_gid, 43);
        assert_eq!(process.fs_gid, 43);
    }

    expect_ok(
        SyscallArgs::new([u64::MAX, 44, 0, 0, 0, 0]).call::<Setreuid>(),
        0,
    );
    {
        let process = get_current_process();
        let process = process.lock();
        assert_eq!(process.real_uid, 42);
        assert_eq!(process.effective_uid, 44);
        assert_eq!(process.saved_uid, 44);
        assert_eq!(process.fs_uid, 44);
    }

    expect_ok(
        SyscallArgs::new([u64::MAX, 45, 0, 0, 0, 0]).call::<Setregid>(),
        0,
    );
    {
        let process = get_current_process();
        let process = process.lock();
        assert_eq!(process.real_gid, 43);
        assert_eq!(process.effective_gid, 45);
        assert_eq!(process.saved_gid, 45);
        assert_eq!(process.fs_gid, 45);
    }

    expect_ok(
        SyscallArgs::new([50, 51, 52, 0, 0, 0]).call::<Setresuid>(),
        0,
    );
    {
        let process = get_current_process();
        let process = process.lock();
        assert_eq!(process.real_uid, 50);
        assert_eq!(process.effective_uid, 51);
        assert_eq!(process.saved_uid, 52);
        assert_eq!(process.fs_uid, 51);
    }

    expect_ok(
        SyscallArgs::new([60, 61, 62, 0, 0, 0]).call::<Setresgid>(),
        0,
    );
    {
        let process = get_current_process();
        let process = process.lock();
        assert_eq!(process.real_gid, 60);
        assert_eq!(process.effective_gid, 61);
        assert_eq!(process.saved_gid, 62);
        assert_eq!(process.fs_gid, 61);
    }

    saved.restore();
}

fn fsuid_fsgid_syscalls_return_previous_ids_and_update_state() {
    let saved = CredentialSnapshot::save_current();

    {
        let process = get_current_process();
        let mut process = process.lock();
        process.fs_uid = 700;
        process.fs_gid = 800;
    }

    expect_ok(
        SyscallArgs::new([701, 0, 0, 0, 0, 0]).call::<Setfsuid>(),
        700,
    );
    expect_ok(
        SyscallArgs::new([801, 0, 0, 0, 0, 0]).call::<Setfsgid>(),
        800,
    );
    {
        let process = get_current_process();
        let process = process.lock();
        assert_eq!(process.fs_uid, 701);
        assert_eq!(process.fs_gid, 801);
    }

    saved.restore();
}

fn group_syscalls_validate_linux_size_rules() {
    let process = get_current_process();
    let saved_groups = process.lock().supplementary_groups.clone();
    let groups = [10u32, 20u32, 30u32];

    expect_ok(
        SyscallArgs::new([groups.len() as u64, groups.as_ptr() as u64, 0, 0, 0, 0])
            .call::<Setgroups>(),
        0,
    );
    expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getgroups>(), 3);
    expect_errno(
        SyscallArgs::new([2, 0, 0, 0, 0, 0]).call::<Getgroups>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<Getgroups>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<Setgroups>(),
        SyscallError::BadAddress,
    );
    expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Setgroups>(), 0);
    assert!(process.lock().supplementary_groups.is_empty());

    process.lock().supplementary_groups = saved_groups;
}

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
