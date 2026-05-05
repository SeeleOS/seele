use crate::{
    memory::protection::Protection,
    misc::timer::ClockId,
    process::{
        ControllingTerminal, Process,
        group::{ProcessGroupID, SessionID},
        manager::get_current_process,
    },
    signal::{Signal, Signals},
    systemcall::{
        arg_types::SyscallArg,
        implementations::{
            Alarm, ArchPrctl, Capget, Capset, ClockGetres, Getegid, Geteuid, Getgid, Getgroups,
            Getpgid, Getpgrp, Getpid, Getppid, Getpriority, Getrandom, Getresgid, Getresuid,
            Getrusage, Getsid, Gettid, Gettimeofday, Getuid, Ioperm, Iopl, IoprioGet, IoprioSet,
            Madvise, OpenFlags, PollEvents, PollTimespec, Prctl, Prlimit64, Reboot, Rseq,
            SchedGetPriorityMax, SchedGetPriorityMin, SchedGetparam, SchedGetscheduler,
            SchedRrGetInterval, SchedSetparam, SchedYield, SelectTimespec, SetRobustList,
            SetTidAddress, Setfsgid, Setfsuid, Setgid, Setgroups, Sethostname, Setpgid,
            Setpriority, Setregid, Setresgid, Setresuid, Setreuid, Setrlimit, Setsid, Settimeofday,
            Setuid, Sync, Sysinfo, Time, Umask, Uname, Vhangup, clear_fdset, fdset_contains,
            fdset_insert, fdset_words, kernel_events_for, saturating_timeout_ms, timeout_is_zero,
            timeout_to_deadline, translate_ready_events,
        },
        linux_semantics::{
            KNOWN_LINUX_SYSCALL_COVERAGE_GAPS, LINUX_SYSCALL_SEMANTICS_COVERAGE,
            LinuxSyscallTestKind,
        },
        numbers::SyscallNumber,
        table::{REGISTERED_SYSCALLS, SYSCALL_TABLE},
        test_helpers::{
            SyscallArgs, allocate_user_test_page, assert_linux_layout, assert_user_bytes,
            errno_code, expect_errno, expect_ok, read_user_value, write_user_value,
        },
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
    scheduler_priority_and_io_permission_syscalls,
    "scheduler priority and io permission syscalls validate linux arguments",
    scheduler_priority_and_io_permission_syscalls_validate_linux_arguments
);
crate::test!(
    process_session_and_prctl_syscalls,
    "process session and prctl syscalls follow linux state rules",
    process_session_and_prctl_syscalls_follow_linux_state_rules
);
crate::test!(
    misc_state_syscalls,
    "misc state syscalls follow linux pointer and state rules",
    misc_state_syscalls_follow_linux_pointer_and_state_rules
);
crate::test!(
    uname_reboot_and_rlimit_syscalls,
    "uname reboot and rlimit syscalls follow linux abi rules",
    uname_reboot_and_rlimit_syscalls_follow_linux_abi_rules
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
    capability_effective: [u32; 2],
    capability_permitted: [u32; 2],
    capability_inheritable: [u32; 2],
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
            capability_effective: process.capability_effective,
            capability_permitted: process.capability_permitted,
            capability_inheritable: process.capability_inheritable,
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
        process.capability_effective = self.capability_effective;
        process.capability_permitted = self.capability_permitted;
        process.capability_inheritable = self.capability_inheritable;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxCapHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxCapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxTimeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxTimezone {
    tz_minuteswest: i32,
    tz_dsttime: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxRusage {
    ru_utime: TestLinuxTimeval,
    ru_stime: TestLinuxTimeval,
    ru_maxrss: i64,
    ru_ixrss: i64,
    ru_idrss: i64,
    ru_isrss: i64,
    ru_minflt: i64,
    ru_majflt: i64,
    ru_nswap: i64,
    ru_inblock: i64,
    ru_oublock: i64,
    ru_msgsnd: i64,
    ru_msgrcv: i64,
    ru_nsignals: i64,
    ru_nvcsw: i64,
    ru_nivcsw: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxSchedParam {
    sched_priority: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxSysinfo {
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
struct TestLinuxRseq {
    cpu_id_start: u32,
    cpu_id: u32,
    rseq_cs: u64,
    flags: u32,
    _padding: u32,
    _padding2: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TestUtsName {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxRlimit64 {
    rlim_cur: u64,
    rlim_max: u64,
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

fn scheduler_priority_and_io_permission_syscalls_validate_linux_arguments() {
    expect_ok(SyscallArgs::none().call::<SchedYield>(), 0);
    expect_ok(SyscallArgs::new([0, 4096, 0, 0, 0, 0]).call::<Madvise>(), 0);
    expect_ok(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getpriority>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([0, 0, 10, 0, 0, 0]).call::<Setpriority>(),
        0,
    );

    expect_ok(
        SyscallArgs::new([1, 0, (2u64 << 13) | 4, 0, 0, 0]).call::<IoprioSet>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<IoprioGet>(),
        2usize << 13,
    );
    expect_errno(
        SyscallArgs::new([1, u64::MAX, 0, 0, 0, 0]).call::<IoprioGet>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([1, 0, (2u64 << 13) | 8, 0, 0, 0]).call::<IoprioSet>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([99, 0, 0, 0, 0, 0]).call::<IoprioGet>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedGetscheduler>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<SchedGetscheduler>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMin>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMin>(),
        1,
    );
    expect_ok(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMax>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMax>(),
        99,
    );
    expect_errno(
        SyscallArgs::new([99, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMax>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Iopl>(), 0);
    expect_ok(SyscallArgs::new([3, 0, 0, 0, 0, 0]).call::<Iopl>(), 0);
    expect_errno(
        SyscallArgs::new([4, 0, 0, 0, 0, 0]).call::<Iopl>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(SyscallArgs::new([0, 1, 1, 0, 0, 0]).call::<Ioperm>(), 0);
}

fn process_session_and_prctl_syscalls_follow_linux_state_rules() {
    let saved = CredentialSnapshot::save_current();
    let process = get_current_process();
    let (
        pid,
        old_group,
        old_session,
        old_terminal,
        old_parent_death_signal,
        old_dumpable,
        old_no_new_privs,
        old_keep_caps,
        old_secure_bits,
        old_bounding,
        old_ambient,
        old_umask,
    ) = {
        let process = process.lock();
        let old_umask = process.fs_context.lock().file_mode_creation_mask;
        (
            process.pid.0,
            process.group_id,
            process.session_id,
            process.controlling_terminal,
            process.parent_death_signal,
            process.dumpable,
            process.no_new_privs,
            process.keep_capabilities,
            process.secure_bits,
            process.capability_bounding,
            process.capability_ambient,
            old_umask,
        )
    };

    {
        let mut process = process.lock();
        process.real_uid = 100;
        process.effective_uid = 101;
        process.saved_uid = 102;
        process.real_gid = 200;
        process.effective_gid = 201;
        process.saved_gid = 202;
    }

    expect_ok(
        SyscallArgs::none().call::<Gettid>(),
        crate::thread::get_current_thread().lock().id.0 as usize,
    );
    expect_ok(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getsid>(),
        old_session.0 as usize,
    );
    expect_errno(
        SyscallArgs::new([u64::from(u32::MAX), 0, 0, 0, 0, 0]).call::<Getsid>(),
        SyscallError::NoProcess,
    );

    expect_ok(
        SyscallArgs::new([0o777, 0, 0, 0, 0, 0]).call::<Umask>(),
        old_umask as usize,
    );
    assert_eq!(
        process.lock().fs_context.lock().file_mode_creation_mask,
        0o777
    );
    expect_ok(
        SyscallArgs::new([0o1000, 0, 0, 0, 0, 0]).call::<Umask>(),
        0o777,
    );
    assert_eq!(process.lock().fs_context.lock().file_mode_creation_mask, 0);

    let uid_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([uid_page, uid_page + 4, uid_page + 8, 0, 0, 0]).call::<Getresuid>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(uid_page), 100);
    assert_eq!(read_user_value::<u32>(uid_page + 4), 101);
    assert_eq!(read_user_value::<u32>(uid_page + 8), 102);
    expect_errno(
        SyscallArgs::new([0, uid_page + 4, uid_page + 8, 0, 0, 0]).call::<Getresuid>(),
        SyscallError::BadAddress,
    );

    let gid_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([gid_page, gid_page + 4, gid_page + 8, 0, 0, 0]).call::<Getresgid>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(gid_page), 200);
    assert_eq!(read_user_value::<u32>(gid_page + 4), 201);
    assert_eq!(read_user_value::<u32>(gid_page + 8), 202);
    expect_errno(
        SyscallArgs::new([gid_page, 0, gid_page + 8, 0, 0, 0]).call::<Getresgid>(),
        SyscallError::BadAddress,
    );

    {
        let mut process = process.lock();
        process.group_id = ProcessGroupID(pid);
    }
    expect_errno(
        SyscallArgs::none().call::<Setsid>(),
        SyscallError::PermissionDenied,
    );
    {
        let mut process = process.lock();
        process.group_id = ProcessGroupID(pid + 7);
        process.session_id = SessionID(pid + 11);
        process.controlling_terminal = Some(ControllingTerminal(123));
    }
    expect_ok(SyscallArgs::none().call::<Setsid>(), pid as usize);
    {
        let process = process.lock();
        assert_eq!(process.group_id, ProcessGroupID(pid));
        assert_eq!(process.session_id, SessionID(pid));
        assert_eq!(process.controlling_terminal, None);
    }

    const PR_SET_PDEATHSIG: u64 = 1;
    const PR_GET_PDEATHSIG: u64 = 2;
    const PR_GET_DUMPABLE: u64 = 3;
    const PR_SET_DUMPABLE: u64 = 4;
    const PR_GET_KEEPCAPS: u64 = 7;
    const PR_SET_KEEPCAPS: u64 = 8;
    const PR_SET_NAME: u64 = 15;
    const PR_GET_NAME: u64 = 16;
    const PR_CAPBSET_READ: u64 = 23;
    const PR_CAPBSET_DROP: u64 = 24;
    const PR_GET_SECUREBITS: u64 = 27;
    const PR_SET_SECUREBITS: u64 = 28;
    const PR_SET_NO_NEW_PRIVS: u64 = 38;
    const PR_GET_NO_NEW_PRIVS: u64 = 39;
    const PR_CAP_AMBIENT: u64 = 47;
    const PR_CAP_AMBIENT_IS_SET: u64 = 1;
    const PR_CAP_AMBIENT_RAISE: u64 = 2;
    const PR_CAP_AMBIENT_LOWER: u64 = 3;
    const PR_CAP_AMBIENT_CLEAR_ALL: u64 = 4;

    expect_ok(
        SyscallArgs::new([PR_SET_PDEATHSIG, Signal::SIGTERM as u64, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    let pdeath_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([PR_GET_PDEATHSIG, pdeath_page, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(pdeath_page), Signal::SIGTERM as i32);
    expect_errno(
        SyscallArgs::new([PR_SET_PDEATHSIG, 999, 0, 0, 0, 0]).call::<Prctl>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([PR_SET_DUMPABLE, 0, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([PR_GET_DUMPABLE, 0, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([PR_SET_DUMPABLE, 2, 0, 0, 0, 0]).call::<Prctl>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(
        SyscallArgs::new([PR_SET_KEEPCAPS, 1, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([PR_GET_KEEPCAPS, 0, 0, 0, 0, 0]).call::<Prctl>(),
        1,
    );
    expect_ok(
        SyscallArgs::new([PR_SET_SECUREBITS, 0x24, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([PR_GET_SECUREBITS, 0, 0, 0, 0, 0]).call::<Prctl>(),
        0x24,
    );
    expect_ok(
        SyscallArgs::new([PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0, 0]).call::<Prctl>(),
        1,
    );
    expect_errno(
        SyscallArgs::new([PR_SET_NO_NEW_PRIVS, 1, 1, 0, 0, 0]).call::<Prctl>(),
        SyscallError::InvalidArguments,
    );

    let name_page = allocate_user_test_page();
    write_user_value(name_page, b"linux-name\0");
    expect_ok(
        SyscallArgs::new([PR_SET_NAME, name_page, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    let out_name_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([PR_GET_NAME, out_name_page, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    assert_user_bytes(out_name_page, b"linux-name\0\0\0\0\0\0");

    expect_ok(
        SyscallArgs::new([PR_CAPBSET_READ, 1, 0, 0, 0, 0]).call::<Prctl>(),
        1,
    );
    expect_ok(
        SyscallArgs::new([PR_CAPBSET_DROP, 1, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([PR_CAPBSET_READ, 1, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([PR_CAPBSET_READ, 64, 0, 0, 0, 0]).call::<Prctl>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_RAISE, 2, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_IS_SET, 2, 0, 0, 0]).call::<Prctl>(),
        1,
    );
    expect_ok(
        SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_LOWER, 2, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_IS_SET, 2, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0, 0]).call::<Prctl>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([999, 0, 0, 0, 0, 0]).call::<Prctl>(),
        SyscallError::InvalidArguments,
    );

    const ARCH_SET_FS: u64 = 0x1002;
    const ARCH_GET_FS: u64 = 0x1003;
    let fs_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([ARCH_GET_FS, fs_page, 0, 0, 0, 0]).call::<ArchPrctl>(),
        0,
    );
    let old_fs_base = read_user_value::<u64>(fs_page);
    expect_ok(
        SyscallArgs::new([ARCH_SET_FS, 0x1234_5000, 0, 0, 0, 0]).call::<ArchPrctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([ARCH_GET_FS, fs_page, 0, 0, 0, 0]).call::<ArchPrctl>(),
        0,
    );
    assert_eq!(read_user_value::<u64>(fs_page), 0x1234_5000);
    expect_ok(
        SyscallArgs::new([ARCH_SET_FS, old_fs_base, 0, 0, 0, 0]).call::<ArchPrctl>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([ARCH_GET_FS, 0, 0, 0, 0, 0]).call::<ArchPrctl>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([0x9999, 0, 0, 0, 0, 0]).call::<ArchPrctl>(),
        SyscallError::InvalidArguments,
    );

    {
        let mut process = process.lock();
        process.group_id = old_group;
        process.session_id = old_session;
        process.controlling_terminal = old_terminal;
        process.parent_death_signal = old_parent_death_signal;
        process.dumpable = old_dumpable;
        process.no_new_privs = old_no_new_privs;
        process.keep_capabilities = old_keep_caps;
        process.secure_bits = old_secure_bits;
        process.capability_bounding = old_bounding;
        process.capability_ambient = old_ambient;
        process.fs_context.lock().file_mode_creation_mask = old_umask;
    }
    saved.restore();
}

fn misc_state_syscalls_follow_linux_pointer_and_state_rules() {
    assert_linux_layout::<TestLinuxCapHeader>(8, 4);
    assert_linux_layout::<TestLinuxCapData>(12, 4);
    assert_linux_layout::<TestLinuxTimeval>(16, 8);
    assert_linux_layout::<TestLinuxTimezone>(8, 4);
    assert_linux_layout::<TestLinuxRusage>(144, 8);
    assert_linux_layout::<TestLinuxSchedParam>(4, 4);
    assert_linux_layout::<TestLinuxSysinfo>(112, 8);
    assert_linux_layout::<TestLinuxRseq>(32, 8);

    const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
    const RSEQ_LEN_X86_64: u64 = 32;
    const RSEQ_FLAG_UNREGISTER: u64 = 1;
    const RSEQ_CPU_ID_UNINITIALIZED: u32 = u32::MAX;
    const RSEQ_CPU_ID_SINGLE_CORE: u32 = 0;

    let saved = CredentialSnapshot::save_current();
    let process = get_current_process();
    let current = crate::thread::get_current_thread();
    let (
        old_clear_child_tid,
        old_robust_list_head,
        old_robust_list_len,
        old_rseq_area,
        old_rseq_len,
        old_rseq_flags,
        old_rseq_sig,
    ) = {
        let current = current.lock();
        (
            current.clear_child_tid,
            current.robust_list_head,
            current.robust_list_len,
            current.rseq_area,
            current.rseq_len,
            current.rseq_flags,
            current.rseq_sig,
        )
    };
    let old_timezone = crate::misc::time::timezone();

    {
        let mut process = process.lock();
        process.capability_effective = [0x1111_1111, 0x22];
        process.capability_permitted = [0x3333_3333, 0x44];
        process.capability_inheritable = [0x5555_5555, 0x66];
    }

    let cap_page = allocate_user_test_page();
    write_user_value(cap_page, &TestLinuxCapHeader { version: 0, pid: 0 });
    expect_ok(
        SyscallArgs::new([cap_page, cap_page + 16, 0, 0, 0, 0]).call::<Capget>(),
        0,
    );
    let header = read_user_value::<TestLinuxCapHeader>(cap_page);
    assert_eq!(header.version, LINUX_CAPABILITY_VERSION_3);
    assert_eq!(header.pid, 0);
    let cap0 = read_user_value::<TestLinuxCapData>(cap_page + 16);
    let cap1 = read_user_value::<TestLinuxCapData>(cap_page + 28);
    assert_eq!(cap0.effective, 0x1111_1111);
    assert_eq!(cap0.permitted, 0x3333_3333);
    assert_eq!(cap0.inheritable, 0x5555_5555);
    assert_eq!(cap1.effective, 0x22);
    assert_eq!(cap1.permitted, 0x44);
    assert_eq!(cap1.inheritable, 0x66);
    expect_errno(
        SyscallArgs::new([0, cap_page + 16, 0, 0, 0, 0]).call::<Capget>(),
        SyscallError::BadAddress,
    );

    write_user_value(
        cap_page,
        &TestLinuxCapHeader {
            version: LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        },
    );
    let new_caps = [
        TestLinuxCapData {
            effective: 0xaa,
            permitted: 0xbb,
            inheritable: 0xcc,
        },
        TestLinuxCapData {
            effective: 0xdd,
            permitted: 0xee,
            inheritable: 0xff,
        },
    ];
    write_user_value(cap_page + 16, &new_caps);
    expect_ok(
        SyscallArgs::new([cap_page, cap_page + 16, 0, 0, 0, 0]).call::<Capset>(),
        0,
    );
    {
        let process = process.lock();
        assert_eq!(process.capability_effective, [0xaa, 0xdd]);
        assert_eq!(process.capability_permitted, [0xbb, 0xee]);
        assert_eq!(process.capability_inheritable, [0xcc, 0xff]);
    }
    write_user_value(
        cap_page,
        &TestLinuxCapHeader {
            version: 0x1998_0522,
            pid: 0,
        },
    );
    expect_errno(
        SyscallArgs::new([cap_page, cap_page + 16, 0, 0, 0, 0]).call::<Capset>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, cap_page + 16, 0, 0, 0, 0]).call::<Capset>(),
        SyscallError::BadAddress,
    );

    let tid_page = allocate_user_test_page();
    let tid = crate::thread::get_current_thread().lock().id.0 as i32;
    expect_ok(
        SyscallArgs::new([tid_page, 0, 0, 0, 0, 0]).call::<SetTidAddress>(),
        tid as usize,
    );
    assert_eq!(read_user_value::<i32>(tid_page), tid);
    assert_eq!(
        crate::thread::get_current_thread().lock().clear_child_tid,
        tid_page
    );
    expect_ok(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SetTidAddress>(),
        tid as usize,
    );
    assert_eq!(
        crate::thread::get_current_thread().lock().clear_child_tid,
        0
    );

    expect_ok(
        SyscallArgs::new([0x1234_5000, 24, 0, 0, 0, 0]).call::<SetRobustList>(),
        0,
    );
    {
        let current = crate::thread::get_current_thread();
        let current = current.lock();
        assert_eq!(current.robust_list_head, 0x1234_5000);
        assert_eq!(current.robust_list_len, 24);
    }

    {
        let current = crate::thread::get_current_thread();
        let mut current = current.lock();
        current.rseq_area = 0;
        current.rseq_len = 0;
        current.rseq_flags = 0;
        current.rseq_sig = 0;
    }
    let rseq_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([rseq_page, RSEQ_LEN_X86_64, 0, 0x5305_5305, 0, 0]).call::<Rseq>(),
        0,
    );
    let rseq = read_user_value::<TestLinuxRseq>(rseq_page);
    assert_eq!(rseq.cpu_id_start, RSEQ_CPU_ID_SINGLE_CORE);
    assert_eq!(rseq.cpu_id, RSEQ_CPU_ID_SINGLE_CORE);
    {
        let current = crate::thread::get_current_thread();
        let current = current.lock();
        assert_eq!(current.rseq_area, rseq_page);
        assert_eq!(current.rseq_len, RSEQ_LEN_X86_64 as u32);
        assert_eq!(current.rseq_sig, 0x5305_5305);
    }
    expect_errno(
        SyscallArgs::new([rseq_page, RSEQ_LEN_X86_64, 0, 0x5305_5305, 0, 0]).call::<Rseq>(),
        SyscallError::DeviceOrResourceBusy,
    );
    expect_ok(
        SyscallArgs::new([
            rseq_page,
            RSEQ_LEN_X86_64,
            RSEQ_FLAG_UNREGISTER,
            0x5305_5305,
            0,
            0,
        ])
        .call::<Rseq>(),
        0,
    );
    let rseq = read_user_value::<TestLinuxRseq>(rseq_page);
    assert_eq!(rseq.cpu_id_start, RSEQ_CPU_ID_UNINITIALIZED);
    assert_eq!(rseq.cpu_id, RSEQ_CPU_ID_UNINITIALIZED);
    expect_errno(
        SyscallArgs::new([0, RSEQ_LEN_X86_64, 0, 0, 0, 0]).call::<Rseq>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([rseq_page, 16, 0, 0, 0, 0]).call::<Rseq>(),
        SyscallError::InvalidArguments,
    );

    let random_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([random_page, 16, 0, 0, 0, 0]).call::<Getrandom>(),
        16,
    );
    expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getrandom>(), 0);
    expect_errno(
        SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Getrandom>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([random_page, 1, 8, 0, 0, 0]).call::<Getrandom>(),
        SyscallError::InvalidArguments,
    );

    let time_page = allocate_user_test_page();
    let seconds = SyscallArgs::new([time_page, 0, 0, 0, 0, 0])
        .call::<Time>()
        .expect("time should succeed");
    assert_eq!(read_user_value::<i64>(time_page) as usize, seconds);
    let null_seconds = SyscallArgs::new([0, 0, 0, 0, 0, 0])
        .call::<Time>()
        .expect("time null should succeed");
    assert!(null_seconds >= seconds);

    let tod_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([tod_page, tod_page + 32, 0, 0, 0, 0]).call::<Gettimeofday>(),
        0,
    );
    let timeval = read_user_value::<TestLinuxTimeval>(tod_page);
    assert!(timeval.tv_sec >= 0);
    assert!((0..1_000_000).contains(&timeval.tv_usec));
    let timezone = read_user_value::<TestLinuxTimezone>(tod_page + 32);
    assert_eq!(timezone.tz_minuteswest, old_timezone.0);
    assert_eq!(timezone.tz_dsttime, old_timezone.1);

    let set_time_page = allocate_user_test_page();
    write_user_value(
        set_time_page,
        &TestLinuxTimeval {
            tv_sec: -1,
            tv_usec: 0,
        },
    );
    expect_errno(
        SyscallArgs::new([set_time_page, 0, 0, 0, 0, 0]).call::<Settimeofday>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(
        set_time_page + 32,
        &TestLinuxTimezone {
            tz_minuteswest: 90,
            tz_dsttime: 1,
        },
    );
    expect_ok(
        SyscallArgs::new([0, set_time_page + 32, 0, 0, 0, 0]).call::<Settimeofday>(),
        0,
    );
    assert_eq!(crate::misc::time::timezone(), (90, 1));
    expect_ok(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Settimeofday>(),
        0,
    );

    let rusage_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([0, rusage_page, 0, 0, 0, 0]).call::<Getrusage>(),
        0,
    );
    assert_eq!(read_user_value::<TestLinuxRusage>(rusage_page).ru_maxrss, 0);
    expect_errno(
        SyscallArgs::new([99, rusage_page, 0, 0, 0, 0]).call::<Getrusage>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getrusage>(),
        SyscallError::BadAddress,
    );

    let sysinfo_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([sysinfo_page, 0, 0, 0, 0, 0]).call::<Sysinfo>(),
        0,
    );
    let info = read_user_value::<TestLinuxSysinfo>(sysinfo_page);
    assert!(info.totalram > 0);
    assert_eq!(info.mem_unit, 1);
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Sysinfo>(),
        SyscallError::BadAddress,
    );

    let sched_page = allocate_user_test_page();
    write_user_value(sched_page, &TestLinuxSchedParam { sched_priority: 0 });
    expect_ok(
        SyscallArgs::new([0, sched_page, 0, 0, 0, 0]).call::<SchedSetparam>(),
        0,
    );
    write_user_value(sched_page, &TestLinuxSchedParam { sched_priority: -1 });
    expect_errno(
        SyscallArgs::new([0, sched_page, 0, 0, 0, 0]).call::<SchedSetparam>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedSetparam>(),
        SyscallError::BadAddress,
    );
    expect_ok(
        SyscallArgs::new([0, sched_page, 0, 0, 0, 0]).call::<SchedGetparam>(),
        0,
    );
    assert_eq!(
        read_user_value::<TestLinuxSchedParam>(sched_page).sched_priority,
        0
    );
    expect_errno(
        SyscallArgs::new([u64::MAX, sched_page, 0, 0, 0, 0]).call::<SchedGetparam>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(SyscallArgs::new([30, 0, 0, 0, 0, 0]).call::<Alarm>(), 0);
    expect_ok(SyscallArgs::none().call::<Sync>(), 0);

    crate::misc::time::set_timezone(old_timezone.0, old_timezone.1);
    {
        let current = crate::thread::get_current_thread();
        let mut current = current.lock();
        current.clear_child_tid = old_clear_child_tid;
        current.robust_list_head = old_robust_list_head;
        current.robust_list_len = old_robust_list_len;
        current.rseq_area = old_rseq_area;
        current.rseq_len = old_rseq_len;
        current.rseq_flags = old_rseq_flags;
        current.rseq_sig = old_rseq_sig;
    }
    saved.restore();
}

fn uname_reboot_and_rlimit_syscalls_follow_linux_abi_rules() {
    assert_linux_layout::<TestUtsName>(390, 1);
    assert_linux_layout::<TestLinuxTimespec>(16, 8);
    assert_linux_layout::<TestLinuxRlimit64>(16, 8);

    const LINUX_REBOOT_MAGIC1: u64 = 0xfee1_dead;
    const LINUX_REBOOT_MAGIC2: u64 = 0x2812_1969;
    const LINUX_REBOOT_CMD_CAD_OFF: u64 = 0x0000_0000;
    const LINUX_REBOOT_CMD_CAD_ON: u64 = 0x89ab_cdef;
    const RLIMIT_STACK: u64 = 3;
    const RLIMIT_NOFILE: u64 = 7;
    const RLIMIT_MEMLOCK: u64 = 8;
    const RLIMIT_RTPRIO: u64 = 14;

    let process = get_current_process();
    let (
        old_stack_cur,
        old_stack_max,
        old_nofile_cur,
        old_nofile_max,
        old_memlock_cur,
        old_memlock_max,
        old_rtprio_cur,
        old_rtprio_max,
    ) = {
        let process = process.lock();
        (
            process.rlimit_stack_cur,
            process.rlimit_stack_max,
            process.rlimit_nofile_cur,
            process.rlimit_nofile_max,
            process.rlimit_memlock_cur,
            process.rlimit_memlock_max,
            process.rlimit_rtprio_cur,
            process.rlimit_rtprio_max,
        )
    };
    let old_cad = crate::misc::reboot::ctrl_alt_del_enabled();

    let host_page = allocate_user_test_page();
    write_user_value(host_page, b"linuxhost");
    expect_ok(
        SyscallArgs::new([host_page, 9, 0, 0, 0, 0]).call::<Sethostname>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([host_page, 65, 0, 0, 0, 0]).call::<Sethostname>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(host_page, b"bad\0host");
    expect_errno(
        SyscallArgs::new([host_page, 8, 0, 0, 0, 0]).call::<Sethostname>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Sethostname>(),
        SyscallError::BadAddress,
    );

    let uts_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([uts_page, 0, 0, 0, 0, 0]).call::<Uname>(),
        0,
    );
    let uts = read_user_value::<TestUtsName>(uts_page);
    assert_eq!(&uts.sysname[..6], b"Seele\0");
    assert_eq!(&uts.nodename[..10], b"linuxhost\0");
    assert_eq!(&uts.machine[..7], b"x86_64\0");
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Uname>(),
        SyscallError::BadAddress,
    );

    expect_errno(
        SyscallArgs::new([0, LINUX_REBOOT_MAGIC2, LINUX_REBOOT_CMD_CAD_OFF, 0, 0, 0])
            .call::<Reboot>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(
        SyscallArgs::new([
            LINUX_REBOOT_MAGIC1,
            LINUX_REBOOT_MAGIC2,
            LINUX_REBOOT_CMD_CAD_OFF,
            0,
            0,
            0,
        ])
        .call::<Reboot>(),
        0,
    );
    assert!(!crate::misc::reboot::ctrl_alt_del_enabled());
    expect_ok(
        SyscallArgs::new([
            LINUX_REBOOT_MAGIC1,
            LINUX_REBOOT_MAGIC2,
            LINUX_REBOOT_CMD_CAD_ON,
            0,
            0,
            0,
        ])
        .call::<Reboot>(),
        0,
    );
    assert!(crate::misc::reboot::ctrl_alt_del_enabled());
    expect_errno(
        SyscallArgs::new([LINUX_REBOOT_MAGIC1, LINUX_REBOOT_MAGIC2, 0x1234, 0, 0, 0])
            .call::<Reboot>(),
        SyscallError::InvalidArguments,
    );

    let timespec_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([0, timespec_page, 0, 0, 0, 0]).call::<SchedRrGetInterval>(),
        0,
    );
    let interval = read_user_value::<TestLinuxTimespec>(timespec_page);
    assert_eq!(interval.tv_sec, 0);
    assert_eq!(interval.tv_nsec, 100_000_000);
    expect_errno(
        SyscallArgs::new([u64::MAX, timespec_page, 0, 0, 0, 0]).call::<SchedRrGetInterval>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedRrGetInterval>(),
        SyscallError::BadAddress,
    );

    let rlimit_page = allocate_user_test_page();
    write_user_value(
        rlimit_page,
        &TestLinuxRlimit64 {
            rlim_cur: 4096,
            rlim_max: 8192,
        },
    );
    expect_ok(
        SyscallArgs::new([RLIMIT_STACK, rlimit_page, 0, 0, 0, 0]).call::<Setrlimit>(),
        0,
    );
    {
        let process = process.lock();
        assert_eq!(process.rlimit_stack_cur, 4096);
        assert_eq!(process.rlimit_stack_max, 8192);
    }
    expect_errno(
        SyscallArgs::new([99, rlimit_page, 0, 0, 0, 0]).call::<Setrlimit>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([RLIMIT_STACK, 0, 0, 0, 0, 0]).call::<Setrlimit>(),
        SyscallError::BadAddress,
    );

    write_user_value(
        rlimit_page,
        &TestLinuxRlimit64 {
            rlim_cur: 256,
            rlim_max: 512,
        },
    );
    expect_ok(
        SyscallArgs::new([0, RLIMIT_NOFILE, rlimit_page, rlimit_page + 32, 0, 0])
            .call::<Prlimit64>(),
        0,
    );
    let old_nofile = read_user_value::<TestLinuxRlimit64>(rlimit_page + 32);
    assert_eq!(old_nofile.rlim_cur, old_nofile_cur);
    assert_eq!(old_nofile.rlim_max, old_nofile_max);
    {
        let process = process.lock();
        assert_eq!(process.rlimit_nofile_cur, 256);
        assert_eq!(process.rlimit_nofile_max, 512);
    }
    expect_ok(
        SyscallArgs::new([0, RLIMIT_MEMLOCK, 0, rlimit_page + 32, 0, 0]).call::<Prlimit64>(),
        0,
    );
    let old_memlock = read_user_value::<TestLinuxRlimit64>(rlimit_page + 32);
    assert_eq!(old_memlock.rlim_cur, old_memlock_cur);
    assert_eq!(old_memlock.rlim_max, old_memlock_max);
    write_user_value(
        rlimit_page,
        &TestLinuxRlimit64 {
            rlim_cur: 7,
            rlim_max: 9,
        },
    );
    expect_ok(
        SyscallArgs::new([0, RLIMIT_RTPRIO, rlimit_page, 0, 0, 0]).call::<Prlimit64>(),
        0,
    );
    {
        let process = process.lock();
        assert_eq!(process.rlimit_rtprio_cur, 7);
        assert_eq!(process.rlimit_rtprio_max, 9);
    }
    expect_errno(
        SyscallArgs::new([1, RLIMIT_NOFILE, 0, 0, 0, 0]).call::<Prlimit64>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, 99, 0, 0, 0, 0]).call::<Prlimit64>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(SyscallArgs::none().call::<Vhangup>(), 0);

    {
        let mut process = process.lock();
        process.rlimit_stack_cur = old_stack_cur;
        process.rlimit_stack_max = old_stack_max;
        process.rlimit_nofile_cur = old_nofile_cur;
        process.rlimit_nofile_max = old_nofile_max;
        process.rlimit_memlock_cur = old_memlock_cur;
        process.rlimit_memlock_max = old_memlock_max;
        process.rlimit_rtprio_cur = old_rtprio_cur;
        process.rlimit_rtprio_max = old_rtprio_max;
    }
    crate::misc::reboot::set_ctrl_alt_del_enabled(old_cad);
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
