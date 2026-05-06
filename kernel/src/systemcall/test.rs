use crate::{
    filesystem::info::LinuxStat,
    filesystem::{absolute_path::AbsolutePath, path::Path, vfs::VirtualFS},
    memory::protection::Protection,
    misc::timer::ClockId,
    object::{FileFlags, misc::get_object_current_process, traits::Statable},
    process::{
        ControllingTerminal, FdFlags, Process,
        group::{ProcessGroupID, SessionID},
        manager::get_current_process,
    },
    signal::{Signal, Signals},
    systemcall::{
        arg_types::SyscallArg,
        implementations::{
            Access, Alarm, ArchPrctl, Capget, Capset, Chdir, Chroot, ClockGetres, ClockGettime,
            ClockNanosleep, ClockSettime, Close, Dup, Dup2, Dup3, Eventfd, Eventfd2, Faccessat,
            Faccessat2, Fadvise64, Fallocate, Fchdir, Fchmod, Fchmodat, Fchown, Fchownat, Fcntl,
            Fdatasync, Fgetxattr, Flistxattr, Flock, Fremovexattr, Fsetxattr, Fstat, Fstatfs,
            Fsync, Ftruncate, Getcwd, Getegid, Geteuid, Getgid, Getgroups, Getpgid, Getpgrp,
            Getpid, Getppid, Getpriority, Getrandom, Getresgid, Getresuid, Getrusage, Getsid,
            Gettid, Gettimeofday, Getuid, Getxattr, InotifyAddWatch, InotifyInit, InotifyInit1,
            InotifyRmWatch, Ioperm, Iopl, IoprioGet, IoprioSet, Lgetxattr, Link, LinkAt, Listxattr,
            Llistxattr, Lremovexattr, Lseek, Lsetxattr, Madvise, MemfdCreate, Mkdir, MkdirAt,
            Mknodat, Newfstatat, Open, OpenAt, OpenFlags, Pipe, Pipe2, PollEvents, PollTimespec,
            Prctl, Pread64, Prlimit64, Pwrite64, Read, Readlink, ReadlinkAt, Reboot, Removexattr,
            Rename, RenameAt, RenameAt2, Rmdir, Rseq, SchedGetPriorityMax, SchedGetPriorityMin,
            SchedGetaffinity, SchedGetparam, SchedGetscheduler, SchedRrGetInterval,
            SchedSetaffinity, SchedSetparam, SchedYield, SelectTimespec, SetRobustList,
            SetTidAddress, Setfsgid, Setfsuid, Setgid, Setgroups, Sethostname, Setpgid,
            Setpriority, Setregid, Setresgid, Setresuid, Setreuid, Setrlimit, Setsid, Settimeofday,
            Setuid, Setxattr, Statfs, Symlink, SymlinkAt, Sync, Sysinfo, Time, TimerfdCreate,
            TimerfdGettime, TimerfdSettime, Umask, Uname, Unlink, UnlinkAt, Vhangup, Write, Writev,
            clear_fdset, fdset_contains, fdset_insert, fdset_words, kernel_events_for,
            saturating_timeout_ms, timeout_is_zero, timeout_to_deadline, translate_ready_events,
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

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct LinuxDirent64Header {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
}

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
    clock_and_affinity_syscalls,
    "clock and affinity syscalls follow linux pointer rules",
    clock_and_affinity_syscalls_follow_linux_pointer_rules
);
crate::test!(
    eventfd_syscalls,
    "eventfd syscalls follow linux flag rules",
    eventfd_syscalls_follow_linux_flag_rules
);
crate::test!(
    inotify_init_syscalls,
    "inotify init syscalls follow linux flag rules",
    inotify_init_syscalls_follow_linux_flag_rules
);
crate::test!(
    timerfd_syscalls,
    "timerfd syscalls follow linux flag and timer rules",
    timerfd_syscalls_follow_linux_flag_and_timer_rules
);
crate::test!(
    pipe_and_dup_syscalls,
    "pipe and dup syscalls follow linux fd rules",
    pipe_and_dup_syscalls_follow_linux_fd_rules
);
crate::test!(
    filesystem_path_state_syscalls,
    "filesystem path state syscalls follow linux rules",
    filesystem_path_state_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_create_link_syscalls,
    "filesystem create link syscalls follow linux rules",
    filesystem_create_link_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_fd_state_syscalls,
    "filesystem fd state syscalls follow linux rules",
    filesystem_fd_state_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_metadata_syscalls,
    "filesystem metadata syscalls follow linux rules",
    filesystem_metadata_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_io_syscalls,
    "filesystem io syscalls follow linux rules",
    filesystem_io_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_rename_syscalls,
    "filesystem rename syscalls follow linux rules",
    filesystem_rename_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_getdents_syscalls,
    "filesystem getdents syscalls follow linux rules",
    filesystem_getdents_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_file_object_syscalls,
    "filesystem file object syscalls follow linux rules",
    filesystem_file_object_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_file_metadata_syscalls,
    "filesystem file metadata syscalls follow linux rules",
    filesystem_file_metadata_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_xattr_syscalls,
    "filesystem xattr syscalls follow linux rules",
    filesystem_xattr_syscalls_follow_linux_rules
);
crate::test!(
    memfd_and_inotify_watch_syscalls,
    "memfd and inotify watch syscalls follow linux rules",
    memfd_and_inotify_watch_syscalls_follow_linux_rules
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
struct TestLinuxItimerspec {
    it_interval: TestLinuxTimespec,
    it_value: TestLinuxTimespec,
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

fn clock_and_affinity_syscalls_follow_linux_pointer_rules() {
    const CLOCK_REALTIME: u64 = 0;
    const CLOCK_MONOTONIC: u64 = 1;
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

fn close_test_fd(fd: usize) {
    let fd_table = get_current_process().lock().fd_table.clone();
    let mut fd_table = fd_table.lock();
    assert!(fd < fd_table.len());
    assert!(fd_table[fd].take().is_some());
}

fn expect_fd(result: Result<usize, SyscallError>) -> usize {
    result.expect("syscall should create a file descriptor")
}

fn assert_fd_flags(fd: usize, expected: FdFlags) {
    let fd_table = get_current_process().lock().fd_table.clone();
    let fd_table = fd_table.lock();
    let flags = fd_table
        .get(fd)
        .and_then(|entry| entry.as_ref())
        .map(|entry| entry.fd_flags)
        .expect("test fd should exist");
    assert_eq!(flags, expected);
}

fn assert_object_flags(fd: usize, expected: FileFlags) {
    let flags = get_object_current_process(fd as u64)
        .expect("test fd should resolve")
        .get_flags()
        .expect("test object should report flags");
    assert_eq!(flags, expected);
}

fn assert_same_object(left_fd: usize, right_fd: usize) {
    let left = get_object_current_process(left_fd as u64).expect("left fd should resolve");
    let right = get_object_current_process(right_fd as u64).expect("right fd should resolve");
    assert!(alloc::sync::Arc::ptr_eq(&left, &right));
}

fn occupied_fd_count() -> usize {
    let fd_table = get_current_process().lock().fd_table.clone();
    fd_table.lock().iter().flatten().count()
}

fn write_user_cstr(addr: u64, value: &[u8]) {
    assert_eq!(value.last(), Some(&0));
    get_current_process()
        .lock()
        .addrspace
        .write_buffer(addr as *mut u8, value)
        .expect("test user c string should be writable");
}

fn eventfd_syscalls_follow_linux_flag_rules() {
    const EFD_SEMAPHORE: u64 = 0x1;
    const EFD_NONBLOCK: u64 = 0o4_000;
    const EFD_CLOEXEC: u64 = 0o2_000_000;

    let eventfd = expect_fd(SyscallArgs::new([7, 0, 0, 0, 0, 0]).call::<Eventfd>());
    assert!(
        get_object_current_process(eventfd as u64)
            .expect("eventfd should resolve")
            .as_eventfd()
            .is_ok()
    );
    assert_fd_flags(eventfd, FdFlags::empty());
    assert_object_flags(eventfd, FileFlags::empty());
    close_test_fd(eventfd);

    let eventfd2 = expect_fd(
        SyscallArgs::new([0, EFD_SEMAPHORE | EFD_NONBLOCK | EFD_CLOEXEC, 0, 0, 0, 0])
            .call::<Eventfd2>(),
    );
    assert_fd_flags(eventfd2, FdFlags::CLOEXEC);
    assert_object_flags(eventfd2, FileFlags::NONBLOCK);
    close_test_fd(eventfd2);
    expect_errno(
        SyscallArgs::new([0, 0x8000_0000, 0, 0, 0, 0]).call::<Eventfd2>(),
        SyscallError::InvalidArguments,
    );
}

fn inotify_init_syscalls_follow_linux_flag_rules() {
    const IN_NONBLOCK: u64 = 0o4_000;
    const IN_CLOEXEC: u64 = 0o2_000_000;

    let inotify = expect_fd(SyscallArgs::none().call::<InotifyInit>());
    assert!(
        get_object_current_process(inotify as u64)
            .expect("inotify fd should resolve")
            .as_inotify()
            .is_ok()
    );
    assert_fd_flags(inotify, FdFlags::empty());
    assert_object_flags(inotify, FileFlags::empty());
    close_test_fd(inotify);

    let inotify1 = expect_fd(
        SyscallArgs::new([IN_NONBLOCK | IN_CLOEXEC, 0, 0, 0, 0, 0]).call::<InotifyInit1>(),
    );
    assert_fd_flags(inotify1, FdFlags::CLOEXEC);
    assert_object_flags(inotify1, FileFlags::NONBLOCK);
    close_test_fd(inotify1);
    expect_errno(
        SyscallArgs::new([0x8000_0000, 0, 0, 0, 0, 0]).call::<InotifyInit1>(),
        SyscallError::InvalidArguments,
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

fn pipe_and_dup_syscalls_follow_linux_fd_rules() {
    const O_NONBLOCK: u64 = 0o4_000;
    const O_CLOEXEC: u64 = 0o2_000_000;

    let fd_page = allocate_user_test_page();

    let occupied_before_bad_pipe = occupied_fd_count();
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Pipe>(),
        SyscallError::BadAddress,
    );
    assert_eq!(occupied_fd_count(), occupied_before_bad_pipe);

    expect_ok(SyscallArgs::new([fd_page, 0, 0, 0, 0, 0]).call::<Pipe>(), 0);
    let pipe_fds = read_user_value::<[i32; 2]>(fd_page);
    let read_fd = pipe_fds[0] as usize;
    let write_fd = pipe_fds[1] as usize;
    assert_ne!(read_fd, write_fd);
    assert!(
        get_object_current_process(read_fd as u64)
            .expect("pipe read fd should resolve")
            .as_unix_socket()
            .is_ok()
    );
    assert!(
        get_object_current_process(write_fd as u64)
            .expect("pipe write fd should resolve")
            .as_unix_socket()
            .is_ok()
    );
    assert_fd_flags(read_fd, FdFlags::empty());
    assert_fd_flags(write_fd, FdFlags::empty());
    assert_object_flags(read_fd, FileFlags::empty());
    assert_object_flags(write_fd, FileFlags::empty());

    expect_ok(
        SyscallArgs::new([fd_page, O_NONBLOCK | O_CLOEXEC, 0, 0, 0, 0]).call::<Pipe2>(),
        0,
    );
    let pipe2_fds = read_user_value::<[i32; 2]>(fd_page);
    let pipe2_read_fd = pipe2_fds[0] as usize;
    let pipe2_write_fd = pipe2_fds[1] as usize;
    assert_ne!(pipe2_read_fd, pipe2_write_fd);
    assert_fd_flags(pipe2_read_fd, FdFlags::CLOEXEC);
    assert_fd_flags(pipe2_write_fd, FdFlags::CLOEXEC);
    assert_object_flags(pipe2_read_fd, FileFlags::NONBLOCK);
    assert_object_flags(pipe2_write_fd, FileFlags::NONBLOCK);
    expect_errno(
        SyscallArgs::new([fd_page, 0x8000_0000, 0, 0, 0, 0]).call::<Pipe2>(),
        SyscallError::InvalidArguments,
    );

    let dup_fd = expect_fd(SyscallArgs::new([pipe2_read_fd as u64, 0, 0, 0, 0, 0]).call::<Dup>());
    assert_same_object(pipe2_read_fd, dup_fd);
    assert_fd_flags(dup_fd, FdFlags::empty());

    let dup2_dest = dup_fd + 5;
    expect_ok(
        SyscallArgs::new([pipe2_read_fd as u64, dup2_dest as u64, 0, 0, 0, 0]).call::<Dup2>(),
        dup2_dest,
    );
    assert_same_object(pipe2_read_fd, dup2_dest);
    assert_fd_flags(dup2_dest, FdFlags::empty());
    expect_ok(
        SyscallArgs::new([pipe2_read_fd as u64, pipe2_read_fd as u64, 0, 0, 0, 0]).call::<Dup2>(),
        pipe2_read_fd,
    );
    expect_errno(
        SyscallArgs::new([u64::MAX, u64::MAX, 0, 0, 0, 0]).call::<Dup2>(),
        SyscallError::BadFileDescriptor,
    );

    let dup3_dest = dup2_dest + 1;
    expect_ok(
        SyscallArgs::new([pipe2_read_fd as u64, dup3_dest as u64, O_CLOEXEC, 0, 0, 0])
            .call::<Dup3>(),
        dup3_dest,
    );
    assert_same_object(pipe2_read_fd, dup3_dest);
    assert_fd_flags(dup3_dest, FdFlags::CLOEXEC);
    expect_errno(
        SyscallArgs::new([pipe2_read_fd as u64, pipe2_read_fd as u64, 0, 0, 0, 0]).call::<Dup3>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([
            pipe2_read_fd as u64,
            (dup3_dest + 1) as u64,
            O_NONBLOCK,
            0,
            0,
            0,
        ])
        .call::<Dup3>(),
        SyscallError::InvalidArguments,
    );

    close_test_fd(dup3_dest);
    close_test_fd(dup2_dest);
    close_test_fd(dup_fd);
    close_test_fd(pipe2_write_fd);
    close_test_fd(pipe2_read_fd);
    close_test_fd(write_fd);
    close_test_fd(read_fd);
}

fn filesystem_path_state_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const AT_EMPTY_PATH: u64 = 0x1000;
    const AT_EACCESS: u64 = 0x200;

    let process = get_current_process();
    let saved_fs_context = process.lock().fs_context.lock().clone();
    let base_path = Path::new("/tmp/syscall-path-state-test");
    let subdir_path = Path::new("/tmp/syscall-path-state-test/subdir");
    let locked_file_path = Path::new("/tmp/syscall-path-state-test/locked");
    let existing_file_path = Path::new("/tmp/syscall-path-state-test/existing");
    let _ = VirtualFS.lock().delete_file(existing_file_path.clone());
    let _ = VirtualFS.lock().delete_file(locked_file_path.clone());
    let _ = VirtualFS.lock().delete_file(subdir_path.clone());
    let _ = VirtualFS.lock().delete_file(base_path.clone());
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS.lock().create_dir(subdir_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(locked_file_path.clone())
        .unwrap();
    VirtualFS
        .lock()
        .open(locked_file_path.clone())
        .unwrap()
        .chmod(0)
        .unwrap();
    VirtualFS
        .lock()
        .create_file(existing_file_path.clone())
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-path-state-test/locked\0");
    expect_ok(
        SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Access>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([user_page, 4, 0, 0, 0, 0]).call::<Access>(),
        SyscallError::AccessDenied,
    );
    expect_errno(
        SyscallArgs::new([user_page, 8, 0, 0, 0, 0]).call::<Access>(),
        SyscallError::InvalidArguments,
    );

    write_user_cstr(user_page, b"/tmp/syscall-path-state-test/existing\0");
    let file_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    expect_ok(
        SyscallArgs::new([file_fd as u64, user_page + 128, 0, AT_EMPTY_PATH, 0, 0])
            .call::<Faccessat>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([
            file_fd as u64,
            user_page + 128,
            0,
            AT_EMPTY_PATH | AT_EACCESS,
            0,
            0,
        ])
        .call::<Faccessat2>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([file_fd as u64, user_page + 128, 0, 0, 0, 0]).call::<Faccessat>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([file_fd as u64, user_page + 128, 0, 0x8000_0000, 0, 0])
            .call::<Faccessat2>(),
        SyscallError::NoSyscall,
    );
    close_test_fd(file_fd);

    write_user_cstr(user_page, b"/tmp/syscall-path-state-test/subdir\0");
    expect_ok(
        SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Chdir>(),
        0,
    );
    {
        let current = process.lock().fs_context.lock().current_directory.clone();
        assert_eq!(current.as_string(), "/tmp/syscall-path-state-test/subdir");
    }
    expect_ok(
        SyscallArgs::new([user_page + 256, 64, 0, 0, 0, 0]).call::<Getcwd>(),
        b"/tmp/syscall-path-state-test/subdir\0".len(),
    );
    assert_user_bytes(user_page + 256, b"/tmp/syscall-path-state-test/subdir\0");
    expect_errno(
        SyscallArgs::new([user_page + 384, 4, 0, 0, 0, 0]).call::<Getcwd>(),
        SyscallError::RangeError,
    );
    expect_errno(
        SyscallArgs::new([0, 64, 0, 0, 0, 0]).call::<Getcwd>(),
        SyscallError::BadAddress,
    );

    let dir_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    write_user_cstr(user_page, b"/tmp/syscall-path-state-test/existing\0");
    let non_dir_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    expect_errno(
        SyscallArgs::new([non_dir_fd as u64, 0, 0, 0, 0, 0]).call::<Fchdir>(),
        SyscallError::NotADirectory,
    );
    expect_ok(
        SyscallArgs::new([dir_fd as u64, 0, 0, 0, 0, 0]).call::<Fchdir>(),
        0,
    );
    {
        let current = process.lock().fs_context.lock().current_directory.clone();
        assert_eq!(current.as_string(), "/tmp/syscall-path-state-test/subdir");
    }
    close_test_fd(non_dir_fd);
    close_test_fd(dir_fd);

    {
        let process = process.lock();
        process.fs_context.lock().current_directory =
            AbsolutePath::from_root_path(&Path::new("/tmp/syscall-path-state-test/subdir"));
    }
    write_user_cstr(user_page, b"/tmp\0");
    expect_ok(
        SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Chroot>(),
        0,
    );
    {
        let fs_context = process.lock().fs_context.lock().clone();
        assert_eq!(fs_context.root_directory.clone().as_string(), "/tmp");
        assert_eq!(
            fs_context
                .current_directory
                .display_string(&fs_context.root_directory),
            "/syscall-path-state-test/subdir"
        );
    }
    write_user_cstr(user_page, b"/syscall-path-state-test/existing\0");
    expect_errno(
        SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Chroot>(),
        SyscallError::NotADirectory,
    );

    {
        *process.lock().fs_context.lock() = saved_fs_context;
    }
    let _ = VirtualFS.lock().delete_file(existing_file_path);
    let _ = VirtualFS.lock().delete_file(locked_file_path);
    let _ = VirtualFS.lock().delete_file(subdir_path);
    let _ = VirtualFS.lock().delete_file(base_path);
}

fn filesystem_create_link_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const AT_EMPTY_PATH: u64 = 0x1000;
    const AT_REMOVEDIR: u64 = 0x200;

    let base_path = Path::new("/tmp/syscall-create-link-test");
    let cleanup_paths = [
        "/tmp/syscall-create-link-test/fdhard",
        "/tmp/syscall-create-link-test/hard",
        "/tmp/syscall-create-link-test/src",
        "/tmp/syscall-create-link-test/atlink",
        "/tmp/syscall-create-link-test/link",
        "/tmp/syscall-create-link-test/nonempty/child",
        "/tmp/syscall-create-link-test/nonempty",
        "/tmp/syscall-create-link-test/atdir",
        "/tmp/syscall-create-link-test/dir",
        "/tmp/syscall-create-link-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-create-link-test/src"))
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-create-link-test/dir\0");
    expect_ok(
        SyscallArgs::new([user_page, 0o755, 0, 0, 0, 0]).call::<Mkdir>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([user_page, 0o755, 0, 0, 0, 0]).call::<Mkdir>(),
        SyscallError::FileAlreadyExists,
    );
    let dir_object = {
        let mut vfs = VirtualFS.lock();
        vfs.open(Path::new("/tmp/syscall-create-link-test/dir"))
            .unwrap()
    };
    let dir_stat = dir_object.stat();
    assert_eq!(dir_stat.st_mode & 0o777, 0o755);

    write_user_cstr(user_page, b"/tmp/syscall-create-link-test/src\0");
    expect_errno(
        SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Rmdir>(),
        SyscallError::NotADirectory,
    );
    write_user_cstr(user_page, b"/tmp/syscall-create-link-test/dir\0");
    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, 0, 0, 0, 0]).call::<UnlinkAt>(),
        SyscallError::IsADirectory,
    );

    VirtualFS
        .lock()
        .create_dir(Path::new("/tmp/syscall-create-link-test/nonempty"))
        .unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-create-link-test/nonempty/child"))
        .unwrap();
    write_user_cstr(user_page, b"/tmp/syscall-create-link-test/nonempty\0");
    expect_errno(
        SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Rmdir>(),
        SyscallError::DirectoryNotEmpty,
    );

    write_user_cstr(user_page, b"/tmp/syscall-create-link-test/dir\0");
    expect_errno(
        SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Unlink>(),
        SyscallError::IsADirectory,
    );
    expect_ok(
        SyscallArgs::new([AT_FDCWD, user_page, AT_REMOVEDIR, 0, 0, 0]).call::<UnlinkAt>(),
        0,
    );

    write_user_cstr(user_page, b"/tmp/syscall-create-link-test/src\0");
    write_user_cstr(user_page + 128, b"/tmp/syscall-create-link-test/hard\0");
    expect_ok(
        SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Link>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Link>(),
        SyscallError::FileAlreadyExists,
    );
    expect_ok(
        SyscallArgs::new([user_page + 128, 0, 0, 0, 0, 0]).call::<Unlink>(),
        0,
    );

    write_user_cstr(user_page, b"/tmp/syscall-create-link-test\0");
    let dir_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::DIRECTORY.bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    write_user_cstr(user_page + 128, b"atdir\0");
    expect_ok(
        SyscallArgs::new([dir_fd as u64, user_page + 128, 0o700, 0, 0, 0]).call::<MkdirAt>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([dir_fd as u64, user_page + 128, AT_REMOVEDIR, 0, 0, 0])
            .call::<UnlinkAt>(),
        0,
    );

    write_user_cstr(user_page, b"/tmp/syscall-create-link-test/src\0");
    let src_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    write_user_cstr(user_page, b"\0");
    write_user_cstr(user_page + 128, b"fdhard\0");
    expect_ok(
        SyscallArgs::new([
            src_fd as u64,
            user_page,
            dir_fd as u64,
            user_page + 128,
            AT_EMPTY_PATH,
            0,
        ])
        .call::<LinkAt>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([dir_fd as u64, user_page + 128, 0, 0, 0, 0]).call::<UnlinkAt>(),
        0,
    );
    close_test_fd(src_fd);

    write_user_cstr(user_page, b"/target/without/nul\0");
    write_user_cstr(user_page + 128, b"/tmp/syscall-create-link-test/link\0");
    expect_ok(
        SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Symlink>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Symlink>(),
        SyscallError::FileAlreadyExists,
    );
    expect_ok(
        SyscallArgs::new([user_page + 128, user_page + 256, 7, 0, 0, 0]).call::<Readlink>(),
        7,
    );
    assert_user_bytes(user_page + 256, b"/target");

    write_user_cstr(user_page, b"relative-target\0");
    write_user_cstr(user_page + 128, b"atlink\0");
    expect_ok(
        SyscallArgs::new([user_page, dir_fd as u64, user_page + 128, 0, 0, 0]).call::<SymlinkAt>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([dir_fd as u64, user_page + 128, user_page + 256, 64, 0, 0])
            .call::<ReadlinkAt>(),
        b"relative-target".len(),
    );
    assert_user_bytes(user_page + 256, b"relative-target");
    expect_errno(
        SyscallArgs::new([dir_fd as u64, user_page + 128, 0x8000_0000, 0, 0, 0]).call::<UnlinkAt>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(
        SyscallArgs::new([dir_fd as u64, user_page + 128, 0, 0, 0, 0]).call::<UnlinkAt>(),
        0,
    );

    close_test_fd(dir_fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_fd_state_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const O_RDONLY: u64 = 0;
    const O_WRONLY: u64 = 1;
    const O_CREAT: u64 = 0x40;
    const O_EXCL: u64 = 0x80;
    const O_TRUNC: u64 = 0x200;
    const O_DIRECTORY: u64 = 0o200000;
    const SEEK_SET: u64 = 0;
    const SEEK_CUR: u64 = 1;
    const SEEK_END: u64 = 2;

    let base_path = Path::new("/tmp/syscall-fd-state-test");
    let cleanup_paths = [
        "/tmp/syscall-fd-state-test/file",
        "/tmp/syscall-fd-state-test/dir",
        "/tmp/syscall-fd-state-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_dir(Path::new("/tmp/syscall-fd-state-test/dir"))
        .unwrap();

    let user_page = allocate_user_test_page();
    let stat_ptr = (user_page + 512) as *mut LinuxStat;

    write_user_cstr(user_page, b"/tmp/syscall-fd-state-test/file\0");
    let create_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            O_WRONLY | O_CREAT | O_EXCL,
            0o640,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    let created_object = get_object_current_process(create_fd as u64).unwrap();
    let created_stat = created_object.as_statable().unwrap().stat();
    assert_eq!(created_stat.st_mode & 0o170000, 0o100000);
    assert_eq!(created_stat.st_mode & 0o777, 0o640);
    expect_errno(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            O_WRONLY | O_CREAT | O_EXCL,
            0o640,
            0,
            0,
        ])
        .call::<OpenAt>(),
        SyscallError::FileAlreadyExists,
    );
    expect_ok(
        SyscallArgs::new([create_fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([create_fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
        SyscallError::BadFileDescriptor,
    );

    let reopen_fd = expect_fd(SyscallArgs::new([user_page, O_RDONLY, 0, 0, 0, 0]).call::<Open>());
    expect_ok(
        SyscallArgs::new([reopen_fd as u64, stat_ptr as u64, 0, 0, 0, 0]).call::<Fstat>(),
        0,
    );
    let linux_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
    assert_eq!(linux_stat.st_mode & 0o170000, 0o100000);
    assert_eq!(linux_stat.st_mode & 0o777, 0o640);
    expect_errno(
        SyscallArgs::new([usize::MAX as u64, stat_ptr as u64, 0, 0, 0, 0]).call::<Fstat>(),
        SyscallError::BadFileDescriptor,
    );

    expect_ok(
        SyscallArgs::new([reopen_fd as u64, 0, SEEK_SET, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([reopen_fd as u64, 0, SEEK_END, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([reopen_fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([reopen_fd as u64, (-1i64) as u64, SEEK_SET, 0, 0, 0]).call::<Lseek>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([reopen_fd as u64, (-1i64) as u64, SEEK_END, 0, 0, 0]).call::<Lseek>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([reopen_fd as u64, 0, 99, 0, 0, 0]).call::<Lseek>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(
        SyscallArgs::new([reopen_fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
        0,
    );

    write_user_cstr(user_page, b"/tmp/syscall-fd-state-test/dir\0");
    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, O_CREAT | O_DIRECTORY, 0o755, 0, 0])
            .call::<OpenAt>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, O_WRONLY | O_TRUNC, 0, 0, 0]).call::<OpenAt>(),
        SyscallError::IsADirectory,
    );
    let dir_fd =
        expect_fd(SyscallArgs::new([AT_FDCWD, user_page, O_DIRECTORY, 0, 0, 0]).call::<OpenAt>());
    expect_ok(
        SyscallArgs::new([dir_fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
        0,
    );
    write_user_cstr(user_page, b"/tmp/syscall-fd-state-test/file\0");
    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, O_DIRECTORY, 0, 0, 0]).call::<OpenAt>(),
        SyscallError::NotADirectory,
    );

    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_metadata_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
    const AT_EMPTY_PATH: u64 = 0x1000;
    const AT_NO_AUTOMOUNT: u64 = 0x800;

    let base_path = Path::new("/tmp/syscall-metadata-test");
    let cleanup_paths = [
        "/tmp/syscall-metadata-test/link",
        "/tmp/syscall-metadata-test/file",
        "/tmp/syscall-metadata-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-metadata-test/file"))
        .unwrap();
    VirtualFS
        .lock()
        .create_symlink(
            Path::new("/tmp/syscall-metadata-test/link"),
            "/tmp/syscall-metadata-test/file",
        )
        .unwrap();

    let user_page = allocate_user_test_page();
    let stat_ptr = (user_page + 512) as *mut LinuxStat;

    write_user_cstr(user_page, b"/tmp/syscall-metadata-test/file\0");
    expect_ok(
        SyscallArgs::new([user_page, 0o640, 0, 0, 0, 0])
            .call::<crate::systemcall::implementations::Chmod>(),
        0,
    );
    let file_object = {
        let mut vfs = VirtualFS.lock();
        vfs.open(Path::new("/tmp/syscall-metadata-test/file"))
            .unwrap()
    };
    assert_eq!(file_object.stat().st_mode & 0o777, 0o640);

    let file_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    expect_ok(
        SyscallArgs::new([file_fd as u64, 0o600, 0, 0, 0, 0]).call::<Fchmod>(),
        0,
    );
    let file_stat_after_fchmod = get_object_current_process(file_fd as u64)
        .unwrap()
        .as_statable()
        .unwrap()
        .stat();
    assert_eq!(file_stat_after_fchmod.st_mode & 0o777, 0o600);

    write_user_cstr(user_page + 128, b"\0");
    expect_ok(
        SyscallArgs::new([file_fd as u64, user_page + 128, 0o644, AT_EMPTY_PATH, 0, 0])
            .call::<crate::systemcall::implementations::Fchmodat2>(),
        0,
    );
    let file_stat_after_empty_path = get_object_current_process(file_fd as u64)
        .unwrap()
        .as_statable()
        .unwrap()
        .stat();
    assert_eq!(file_stat_after_empty_path.st_mode & 0o777, 0o644);

    expect_errno(
        SyscallArgs::new([file_fd as u64, 0, 0o644, 0, 0, 0])
            .call::<crate::systemcall::implementations::Fchmodat2>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([file_fd as u64, user_page + 128, 0o644, 0x4000_0000, 0, 0])
            .call::<crate::systemcall::implementations::Fchmodat2>(),
        SyscallError::InvalidArguments,
    );

    write_user_cstr(user_page, b"/tmp/syscall-metadata-test/link\0");
    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, 0o700, AT_SYMLINK_NOFOLLOW, 0, 0])
            .call::<crate::systemcall::implementations::Fchmodat2>(),
        SyscallError::OperationNotSupported,
    );
    let target_object_after_link_nofollow = {
        let mut vfs = VirtualFS.lock();
        vfs.open(Path::new("/tmp/syscall-metadata-test/file"))
            .unwrap()
    };
    let target_stat_after_link_nofollow = target_object_after_link_nofollow.stat();
    assert_eq!(target_stat_after_link_nofollow.st_mode & 0o777, 0o644);

    expect_ok(
        SyscallArgs::new([AT_FDCWD, user_page, 0o700, 0, 0, 0]).call::<Fchmodat>(),
        0,
    );
    let target_object_after_follow = {
        let mut vfs = VirtualFS.lock();
        vfs.open(Path::new("/tmp/syscall-metadata-test/file"))
            .unwrap()
    };
    let target_stat_after_follow = target_object_after_follow.stat();
    assert_eq!(target_stat_after_follow.st_mode & 0o777, 0o700);

    expect_ok(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            stat_ptr as u64,
            AT_SYMLINK_NOFOLLOW,
            0,
            0,
        ])
        .call::<Newfstatat>(),
        0,
    );
    let symlink_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
    assert_eq!(symlink_stat.st_mode & 0o170000, 0o120000);

    expect_ok(
        SyscallArgs::new([AT_FDCWD, user_page, stat_ptr as u64, 0, 0, 0]).call::<Newfstatat>(),
        0,
    );
    let followed_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
    assert_eq!(followed_stat.st_mode & 0o170000, 0o100000);
    assert_eq!(followed_stat.st_mode & 0o777, 0o700);

    expect_ok(
        SyscallArgs::new([
            file_fd as u64,
            user_page + 128,
            stat_ptr as u64,
            AT_EMPTY_PATH | AT_NO_AUTOMOUNT,
            0,
            0,
        ])
        .call::<Newfstatat>(),
        0,
    );
    let empty_path_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
    assert_eq!(empty_path_stat.st_mode & 0o170000, 0o100000);
    assert_eq!(empty_path_stat.st_mode & 0o777, 0o700);

    expect_errno(
        SyscallArgs::new([file_fd as u64, 0, stat_ptr as u64, 0, 0, 0]).call::<Newfstatat>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([
            file_fd as u64,
            user_page + 128,
            stat_ptr as u64,
            0x4000_0000,
            0,
            0,
        ])
        .call::<Newfstatat>(),
        SyscallError::NoSyscall,
    );

    close_test_fd(file_fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_io_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let base_path = Path::new("/tmp/syscall-io-test");
    let cleanup_paths = ["/tmp/syscall-io-test/file", "/tmp/syscall-io-test"];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-io-test/file"))
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-io-test/file\0");
    let fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );

    get_object_current_process(fd as u64)
        .unwrap()
        .as_file_like()
        .unwrap()
        .truncate(0)
        .unwrap();

    get_current_process()
        .lock()
        .addrspace
        .write_buffer(user_page as *mut u8, b"abcdef")
        .unwrap();
    expect_ok(
        SyscallArgs::new([fd as u64, user_page, 6, 0, 0, 0]).call::<Write>(),
        6,
    );

    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((user_page + 128) as *mut u8, &[0; 6])
        .unwrap();
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 128, 6, 0, 0, 0]).call::<Read>(),
        6,
    );
    assert_user_bytes(user_page + 128, b"abcdef");

    get_current_process()
        .lock()
        .addrspace
        .write_buffer((user_page + 256) as *mut u8, b"ZZ")
        .unwrap();
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 256, 2, 2, 0, 0]).call::<Pwrite64>(),
        2,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((user_page + 384) as *mut u8, &[0; 6])
        .unwrap();
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 384, 6, 0, 0, 0]).call::<Read>(),
        6,
    );
    assert_user_bytes(user_page + 384, b"abZZef");

    get_current_process()
        .lock()
        .addrspace
        .write_buffer((user_page + 512) as *mut u8, &[0; 3])
        .unwrap();
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 512, 3, 1, 0, 0]).call::<Pread64>(),
        3,
    );
    assert_user_bytes(user_page + 512, b"bZZ");

    let current_offset = get_object_current_process(fd as u64)
        .unwrap()
        .as_seekable()
        .unwrap()
        .seek(0, crate::filesystem::vfs_traits::Whence::Current)
        .unwrap();
    assert_eq!(current_offset, 6);

    expect_errno(
        SyscallArgs::new([fd as u64, user_page + 640, 1, (-1i64) as u64, 0, 0]).call::<Pread64>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, user_page + 640, 1, (-1i64) as u64, 0, 0]).call::<Pwrite64>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, 0, 1, 0, 0, 0]).call::<Read>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, 0, 1, 0, 0, 0]).call::<Write>(),
        SyscallError::BadAddress,
    );

    close_test_fd(fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_rename_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let base_path = Path::new("/tmp/syscall-rename-test");
    let cleanup_paths = [
        "/tmp/syscall-rename-test/dst",
        "/tmp/syscall-rename-test/src",
        "/tmp/syscall-rename-test/subdir/child",
        "/tmp/syscall-rename-test/subdir/renamed",
        "/tmp/syscall-rename-test/subdir",
        "/tmp/syscall-rename-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-rename-test/src"))
        .unwrap();
    VirtualFS
        .lock()
        .create_dir(Path::new("/tmp/syscall-rename-test/subdir"))
        .unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-rename-test/subdir/child"))
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-rename-test/src\0");
    write_user_cstr(user_page + 128, b"/tmp/syscall-rename-test/dst\0");
    expect_ok(
        SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Rename>(),
        0,
    );
    {
        let mut vfs = VirtualFS.lock();
        assert!(vfs.open(Path::new("/tmp/syscall-rename-test/dst")).is_ok());
        assert!(matches!(
            vfs.open(Path::new("/tmp/syscall-rename-test/src")),
            Err(crate::filesystem::errors::FSError::NotFound)
        ));
    }

    expect_ok(
        SyscallArgs::new([user_page + 128, user_page + 128, 0, 0, 0, 0]).call::<Rename>(),
        0,
    );

    write_user_cstr(user_page, b"/tmp/syscall-rename-test/missing\0");
    expect_errno(
        SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Rename>(),
        SyscallError::FileNotFound,
    );

    write_user_cstr(user_page, b"/tmp/syscall-rename-test\0");
    let dir_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::DIRECTORY.bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    write_user_cstr(user_page, b"subdir/child\0");
    write_user_cstr(user_page + 128, b"subdir/renamed\0");
    expect_ok(
        SyscallArgs::new([
            dir_fd as u64,
            user_page,
            dir_fd as u64,
            user_page + 128,
            0,
            0,
        ])
        .call::<RenameAt>(),
        0,
    );
    {
        let mut vfs = VirtualFS.lock();
        assert!(
            vfs.open(Path::new("/tmp/syscall-rename-test/subdir/renamed"))
                .is_ok()
        );
        assert!(matches!(
            vfs.open(Path::new("/tmp/syscall-rename-test/subdir/child")),
            Err(crate::filesystem::errors::FSError::NotFound)
        ));
    }

    expect_errno(
        SyscallArgs::new([
            dir_fd as u64,
            user_page,
            dir_fd as u64,
            user_page + 128,
            1,
            0,
        ])
        .call::<RenameAt2>(),
        SyscallError::NoSyscall,
    );
    close_test_fd(dir_fd);

    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_getdents_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const DT_DIR: u8 = 4;
    const DT_REG: u8 = 8;
    const DT_LNK: u8 = 10;

    let base_path = Path::new("/tmp/syscall-getdents-test");
    let cleanup_paths = [
        "/tmp/syscall-getdents-test/file",
        "/tmp/syscall-getdents-test/link",
        "/tmp/syscall-getdents-test/subdir",
        "/tmp/syscall-getdents-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-getdents-test/file"))
        .unwrap();
    VirtualFS
        .lock()
        .create_dir(Path::new("/tmp/syscall-getdents-test/subdir"))
        .unwrap();
    VirtualFS
        .lock()
        .create_symlink(Path::new("/tmp/syscall-getdents-test/link"), "file")
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-getdents-test\0");
    let dir_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::DIRECTORY.bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );

    expect_errno(
        SyscallArgs::new([dir_fd as u64, 0, 256, 0, 0, 0])
            .call::<crate::systemcall::implementations::Getdents64>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([dir_fd as u64, user_page + 128, 8, 0, 0, 0])
            .call::<crate::systemcall::implementations::Getdents64>(),
        SyscallError::InvalidArguments,
    );

    let bytes_result = SyscallArgs::new([dir_fd as u64, user_page + 128, 512, 0, 0, 0])
        .call::<crate::systemcall::implementations::Getdents64>();
    let bytes = bytes_result.expect("getdents64 should return byte count");
    assert!(bytes > 0);

    let mut offset = 0usize;
    let mut saw_file = false;
    let mut saw_dir = false;
    let mut saw_link = false;
    while offset < bytes {
        let entry =
            read_user_value::<LinuxDirent64Header>((user_page + 128 + offset as u64) as u64);
        assert!(entry.d_reclen as usize >= 24);
        assert!(entry.d_off >= 1);
        let name_len = entry.d_reclen as usize - 19;
        let raw_name = get_current_process()
            .lock()
            .addrspace
            .read_buffer(
                (user_page + 128 + offset as u64 + 19) as *const u8,
                name_len,
            )
            .unwrap();
        let nul = raw_name.iter().position(|byte| *byte == 0).unwrap();
        let name = core::str::from_utf8(&raw_name[..nul]).unwrap();
        match (name, entry.d_type) {
            ("file", DT_REG) => saw_file = true,
            ("subdir", DT_DIR) => saw_dir = true,
            ("link", DT_LNK) => saw_link = true,
            _ => {}
        }
        offset += entry.d_reclen as usize;
    }
    assert!(saw_file);
    assert!(saw_dir);
    assert!(saw_link);

    expect_ok(
        SyscallArgs::new([dir_fd as u64, user_page + 128, 512, 0, 0, 0])
            .call::<crate::systemcall::implementations::Getdents>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([dir_fd as u64, user_page + 128, 8, 0, 0, 0])
            .call::<crate::systemcall::implementations::Getdents64>(),
        0,
    );

    close_test_fd(dir_fd);
    expect_errno(
        SyscallArgs::new([dir_fd as u64, user_page + 128, 512, 0, 0, 0])
            .call::<crate::systemcall::implementations::Getdents64>(),
        SyscallError::BadFileDescriptor,
    );

    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_file_object_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const O_APPEND: u64 = 0o2_000;
    const LOCK_SH: u64 = 1;
    const LOCK_EX: u64 = 2;
    const LOCK_NB: u64 = 4;
    const LOCK_UN: u64 = 8;
    const F_GETFD: u64 = 1;
    const F_SETFD: u64 = 2;
    const F_GETFL: u64 = 3;
    const F_SETFL: u64 = 4;
    const FD_CLOEXEC: u64 = 1;
    const POSIX_FADV_RANDOM: u64 = 1;
    const FALLOC_FL_KEEP_SIZE: u64 = 0x01;
    const FALLOC_FL_PUNCH_HOLE: u64 = 0x02;

    let base_path = Path::new("/tmp/syscall-file-object-test");
    let cleanup_paths = [
        "/tmp/syscall-file-object-test/file",
        "/tmp/syscall-file-object-test/out",
        "/tmp/syscall-file-object-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-file-object-test/file"))
        .unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-file-object-test/out"))
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-file-object-test/file\0");
    let fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    write_user_cstr(user_page + 128, b"/tmp/syscall-file-object-test/file\0");
    let out_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page + 128,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );

    get_object_current_process(fd as u64)
        .unwrap()
        .as_file_like()
        .unwrap()
        .truncate(0)
        .unwrap();
    get_object_current_process(out_fd as u64)
        .unwrap()
        .as_file_like()
        .unwrap()
        .truncate(0)
        .unwrap();

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TestLinuxIovec {
        iov_base: *const u8,
        iov_len: usize,
    }

    let chunk_a = user_page + 256;
    let chunk_b = user_page + 320;
    get_current_process()
        .lock()
        .addrspace
        .write_buffer(chunk_a as *mut u8, b"ab")
        .unwrap();
    get_current_process()
        .lock()
        .addrspace
        .write_buffer(chunk_b as *mut u8, b"cdef")
        .unwrap();
    write_user_value(
        user_page + 384,
        &[
            TestLinuxIovec {
                iov_base: chunk_a as *const u8,
                iov_len: 2,
            },
            TestLinuxIovec {
                iov_base: chunk_b as *const u8,
                iov_len: 4,
            },
        ],
    );
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 384, 2, 0, 0, 0]).call::<Writev>(),
        6,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((user_page + 512) as *mut u8, &[0; 6])
        .unwrap();
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 512, 6, 0, 0, 0]).call::<Read>(),
        6,
    );
    assert_user_bytes(user_page + 512, b"abcdef");

    expect_errno(
        SyscallArgs::new([fd as u64, 0, 1, 0, 0, 0]).call::<Writev>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, user_page + 384, u64::MAX, 0, 0, 0]).call::<Writev>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(
        user_page + 448,
        &[TestLinuxIovec {
            iov_base: core::ptr::null(),
            iov_len: 1,
        }],
    );
    expect_errno(
        SyscallArgs::new([fd as u64, user_page + 448, 1, 0, 0, 0]).call::<Writev>(),
        SyscallError::BadAddress,
    );

    expect_ok(
        SyscallArgs::new([fd as u64, F_GETFD, 0, 0, 0, 0]).call::<Fcntl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, F_SETFD, FD_CLOEXEC, 0, 0, 0]).call::<Fcntl>(),
        0,
    );
    assert_fd_flags(fd, FdFlags::CLOEXEC);
    expect_ok(
        SyscallArgs::new([fd as u64, F_GETFD, 0, 0, 0, 0]).call::<Fcntl>(),
        1,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, F_SETFL, O_APPEND, 0, 0, 0]).call::<Fcntl>(),
        0,
    );
    assert_object_flags(fd, FileFlags::APPEND);
    assert_eq!(
        SyscallArgs::new([fd as u64, F_GETFL, 0, 0, 0, 0])
            .call::<Fcntl>()
            .unwrap() as u64
            & O_APPEND,
        O_APPEND
    );
    expect_errno(
        SyscallArgs::new([fd as u64, 9999, 0, 0, 0, 0]).call::<Fcntl>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([fd as u64, LOCK_EX | LOCK_NB, 0, 0, 0, 0]).call::<Flock>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([out_fd as u64, LOCK_EX | LOCK_NB, 0, 0, 0, 0]).call::<Flock>(),
        SyscallError::TryAgain,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, LOCK_UN, 0, 0, 0, 0]).call::<Flock>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([out_fd as u64, LOCK_SH | LOCK_NB, 0, 0, 0, 0]).call::<Flock>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, LOCK_EX | LOCK_NB, 0, 0, 0, 0]).call::<Flock>(),
        SyscallError::TryAgain,
    );
    expect_ok(
        SyscallArgs::new([out_fd as u64, LOCK_UN, 0, 0, 0, 0]).call::<Flock>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Flock>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([fd as u64, 2, 0, 0, 0, 0]).call::<Ftruncate>(),
        0,
    );
    let truncated_stat = get_object_current_process(fd as u64)
        .unwrap()
        .as_statable()
        .unwrap()
        .stat();
    assert_eq!(truncated_stat.st_size, 2);
    expect_errno(
        SyscallArgs::new([fd as u64, (-1i64) as u64, 0, 0, 0, 0]).call::<Ftruncate>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, POSIX_FADV_RANDOM, 0, 0]).call::<Fadvise64>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, 0, 0, 6, 0, 0]).call::<Fadvise64>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([fd as u64, 0, 1, 2, 0, 0]).call::<Fallocate>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([
            fd as u64,
            FALLOC_FL_KEEP_SIZE | FALLOC_FL_PUNCH_HOLE,
            0,
            0,
            0,
            0,
        ])
        .call::<Fallocate>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, 0x10, 0, 1, 0, 0]).call::<Fallocate>(),
        SyscallError::OperationNotSupported,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, 0, (-1i64) as u64, 1, 0, 0]).call::<Fallocate>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Fsync>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Fdatasync>(),
        0,
    );

    close_test_fd(out_fd);
    close_test_fd(fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_file_metadata_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
    const TMPFS_MAGIC: i64 = 0x0102_1994;

    let base_path = Path::new("/tmp/syscall-file-metadata-test");
    let cleanup_paths = [
        "/tmp/syscall-file-metadata-test/link",
        "/tmp/syscall-file-metadata-test/file",
        "/tmp/syscall-file-metadata-test/node",
        "/tmp/syscall-file-metadata-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-file-metadata-test/file"))
        .unwrap();
    VirtualFS
        .lock()
        .create_symlink(
            Path::new("/tmp/syscall-file-metadata-test/link"),
            "/tmp/syscall-file-metadata-test/file",
        )
        .unwrap();

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestLinuxStatFs {
        f_type: i64,
        f_bsize: i64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_fsid: i64,
        f_namelen: i64,
        f_frsize: i64,
        f_flags: i64,
        f_spare: [i64; 4],
    }

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-file-metadata-test/file\0");
    let fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );

    expect_ok(
        SyscallArgs::new([user_page, user_page + 256, 0, 0, 0, 0]).call::<Statfs>(),
        0,
    );
    let statfs = read_user_value::<TestLinuxStatFs>(user_page + 256);
    assert_eq!(statfs.f_type, TMPFS_MAGIC);
    assert_eq!(statfs.f_bsize, 4096);
    assert_eq!(statfs.f_namelen, 255);

    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 384, 0, 0, 0, 0]).call::<Fstatfs>(),
        0,
    );
    let fstatfs = read_user_value::<TestLinuxStatFs>(user_page + 384);
    assert_eq!(fstatfs.f_type, TMPFS_MAGIC);
    expect_errno(
        SyscallArgs::new([4096, user_page + 384, 0, 0, 0, 0]).call::<Fstatfs>(),
        SyscallError::BadFileDescriptor,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Fstatfs>(),
        SyscallError::BadAddress,
    );

    expect_ok(
        SyscallArgs::new([user_page, 123, 456, 0, 0, 0])
            .call::<crate::systemcall::implementations::Chown>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, 123, 456, 0, 0, 0]).call::<Fchown>(),
        0,
    );
    write_user_cstr(user_page + 128, b"/tmp/syscall-file-metadata-test/link\0");
    expect_ok(
        SyscallArgs::new([AT_FDCWD, user_page + 128, 1, 2, AT_SYMLINK_NOFOLLOW, 0])
            .call::<Fchownat>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([AT_FDCWD, 0, 1, 2, 0, 0]).call::<Fchownat>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, user_page + 128, 1, 2, 0x4000_0000, 0]).call::<Fchownat>(),
        SyscallError::InvalidArguments,
    );

    write_user_cstr(user_page + 192, b"/tmp/syscall-file-metadata-test/node\0");
    expect_ok(
        SyscallArgs::new([AT_FDCWD, user_page + 192, 0o100600, 0, 0, 0]).call::<Mknodat>(),
        0,
    );
    let node_object = {
        let mut vfs = VirtualFS.lock();
        vfs.open(Path::new("/tmp/syscall-file-metadata-test/node"))
            .unwrap()
    };
    assert_eq!(node_object.stat().st_mode & 0o170000, 0o100000);
    assert_eq!(node_object.stat().st_mode & 0o777, 0o600);
    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page + 192, 0o040755, 0, 0, 0]).call::<Mknodat>(),
        SyscallError::NoSyscall,
    );

    close_test_fd(fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_xattr_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const XATTR_CREATE: u64 = 0x1;
    const XATTR_REPLACE: u64 = 0x2;

    let base_path = Path::new("/tmp/syscall-xattr-test");
    let cleanup_paths = [
        "/tmp/syscall-xattr-test/link",
        "/tmp/syscall-xattr-test/file",
        "/tmp/syscall-xattr-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-xattr-test/file"))
        .unwrap();
    VirtualFS
        .lock()
        .create_symlink(
            Path::new("/tmp/syscall-xattr-test/link"),
            "/tmp/syscall-xattr-test/file",
        )
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-xattr-test/file\0");
    write_user_cstr(user_page + 128, b"user.test\0");
    let fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            OpenFlags::empty().bits() as u64,
            0,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );

    expect_ok(
        SyscallArgs::new([user_page, user_page + 128, user_page + 256, 4, 0, 0]).call::<Setxattr>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([
            user_page,
            user_page + 128,
            user_page + 256,
            4,
            XATTR_CREATE,
            0,
        ])
        .call::<Setxattr>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([
            user_page,
            user_page + 128,
            user_page + 256,
            4,
            XATTR_REPLACE,
            0,
        ])
        .call::<Setxattr>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([
            user_page,
            user_page + 128,
            user_page + 256,
            4,
            XATTR_CREATE | XATTR_REPLACE,
            0,
        ])
        .call::<Setxattr>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([user_page, user_page + 128, user_page + 256, 4, 0x4, 0])
            .call::<Setxattr>(),
        SyscallError::InvalidArguments,
    );

    write_user_cstr(user_page + 64, b"/tmp/syscall-xattr-test/link\0");
    expect_ok(
        SyscallArgs::new([user_page + 64, user_page + 128, user_page + 256, 4, 0, 0])
            .call::<Lsetxattr>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 128, user_page + 256, 4, 0, 0])
            .call::<Fsetxattr>(),
        0,
    );

    expect_errno(
        SyscallArgs::new([user_page, user_page + 128, user_page + 384, 16, 0, 0])
            .call::<Getxattr>(),
        SyscallError::NoData,
    );
    expect_errno(
        SyscallArgs::new([user_page + 64, user_page + 128, user_page + 384, 16, 0, 0])
            .call::<Lgetxattr>(),
        SyscallError::NoData,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, user_page + 128, user_page + 384, 16, 0, 0])
            .call::<Fgetxattr>(),
        SyscallError::NoData,
    );

    expect_ok(
        SyscallArgs::new([user_page, user_page + 512, 0, 0, 0, 0]).call::<Listxattr>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([user_page + 64, user_page + 512, 0, 0, 0, 0]).call::<Llistxattr>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 512, 0, 0, 0, 0]).call::<Flistxattr>(),
        0,
    );

    expect_errno(
        SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Removexattr>(),
        SyscallError::NoData,
    );
    expect_errno(
        SyscallArgs::new([user_page + 64, user_page + 128, 0, 0, 0, 0]).call::<Lremovexattr>(),
        SyscallError::NoData,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, user_page + 128, 0, 0, 0, 0]).call::<Fremovexattr>(),
        SyscallError::NoData,
    );

    write_user_cstr(user_page + 192, b"/tmp/syscall-xattr-test/missing\0");
    expect_errno(
        SyscallArgs::new([user_page + 192, user_page + 128, user_page + 256, 4, 0, 0])
            .call::<Setxattr>(),
        SyscallError::FileNotFound,
    );
    expect_errno(
        SyscallArgs::new([user_page + 192, user_page + 128, user_page + 384, 16, 0, 0])
            .call::<Getxattr>(),
        SyscallError::FileNotFound,
    );

    close_test_fd(fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn memfd_and_inotify_watch_syscalls_follow_linux_rules() {
    const MFD_CLOEXEC: u64 = 0x0001;
    const MFD_ALLOW_SEALING: u64 = 0x0002;
    const MFD_NOEXEC_SEAL: u64 = 0x0008;
    const MFD_EXEC: u64 = 0x0010;

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"demo/memfd\0");
    let memfd = expect_fd(
        SyscallArgs::new([user_page, MFD_CLOEXEC | MFD_ALLOW_SEALING, 0, 0, 0, 0])
            .call::<MemfdCreate>(),
    );
    assert_fd_flags(memfd, FdFlags::CLOEXEC);
    let memfd_stat = get_object_current_process(memfd as u64)
        .unwrap()
        .as_statable()
        .unwrap()
        .stat();
    assert_eq!(memfd_stat.st_mode & 0o170000, 0o100000);
    assert_eq!(memfd_stat.st_mode & 0o777, 0o600);

    expect_ok(
        SyscallArgs::new([memfd as u64, 1034, 0, 0, 0, 0]).call::<Fcntl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([memfd as u64, 1033, 0x0002, 0, 0, 0]).call::<Fcntl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([memfd as u64, 1034, 0, 0, 0, 0]).call::<Fcntl>(),
        0x0002,
    );

    expect_errno(
        SyscallArgs::new([user_page, 0x4, 0, 0, 0, 0]).call::<MemfdCreate>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([user_page, MFD_NOEXEC_SEAL | MFD_EXEC, 0, 0, 0, 0]).call::<MemfdCreate>(),
        SyscallError::InvalidArguments,
    );

    let inotify = expect_fd(SyscallArgs::none().call::<InotifyInit>());
    write_user_cstr(user_page + 128, b"/tmp\0");
    let wd1 = SyscallArgs::new([inotify as u64, user_page + 128, 0xffff_ffff, 0, 0, 0])
        .call::<InotifyAddWatch>()
        .expect("inotify_add_watch should succeed");
    let wd2 = SyscallArgs::new([inotify as u64, user_page + 128, 0, 0, 0, 0])
        .call::<InotifyAddWatch>()
        .expect("second watch should succeed");
    assert!(wd2 > wd1);
    expect_ok(
        SyscallArgs::new([inotify as u64, wd1 as u64, 0, 0, 0, 0]).call::<InotifyRmWatch>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([inotify as u64, wd2 as u64, 0, 0, 0, 0]).call::<InotifyRmWatch>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([memfd as u64, user_page + 128, 0, 0, 0, 0]).call::<InotifyAddWatch>(),
        SyscallError::BadFileDescriptor,
    );
    expect_errno(
        SyscallArgs::new([memfd as u64, 1, 0, 0, 0, 0]).call::<InotifyRmWatch>(),
        SyscallError::BadFileDescriptor,
    );

    close_test_fd(inotify);
    close_test_fd(memfd);
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
