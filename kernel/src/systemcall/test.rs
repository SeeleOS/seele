use crate::{
    filesystem::info::LinuxStat,
    filesystem::{absolute_path::AbsolutePath, path::Path, vfs::VirtualFS},
    ipc::sysv_shm::LinuxShmidDs,
    memory::{addrspace::mem_area::Data, protection::Protection},
    misc::{signal::send_signal_to_process_with_siginfo, timer::ClockId},
    object::{FileFlags, config::LinuxTermios, misc::get_object_current_process, traits::Statable},
    process::{
        ControllingTerminal, FdFlags, Process, ProcessExitStatus,
        group::{ProcessGroupID, SessionID},
        manager::{MANAGER, get_current_process},
        misc::ProcessID,
    },
    signal::{SigInfo, Signal, Signals},
    smp::set_current_process,
    systemcall::{
        arg_types::SyscallArg,
        implementations::{
            Accept, Accept4, Access, AddKey, Alarm, ArchPrctl, Bind, Bpf, Brk, Capget, Capset,
            Chdir, Chroot, ClockGetres, ClockGettime, ClockNanosleep, ClockSettime, Clone, Clone3,
            Close, CloseRange, Connect, CopyFileRange, CreatePty, Dup, Dup2, Dup3, EpollCreate1,
            EpollCtl, EpollPwait, EpollPwait2, EpollWait, Eventfd, Eventfd2, Execve, Faccessat,
            Faccessat2, Fadvise64, Fallocate, Fchdir, Fchmod, Fchmodat, Fchown, Fchownat, Fcntl,
            Fdatasync, Fgetxattr, Flistxattr, Flock, Fremovexattr, Fsconfig, Fsetxattr, Fsmount,
            Fsopen, Fstat, Fstatfs, Fsync, Ftruncate, Futex, Getcwd, Getegid, Geteuid, Getgid,
            Getgroups, Getpeername, Getpgid, Getpgrp, Getpid, Getppid, Getpriority, Getrandom,
            Getresgid, Getresuid, Getrusage, Getsid, Getsockname, Getsockopt, Gettid, Gettimeofday,
            Getuid, Getxattr, InotifyAddWatch, InotifyInit, InotifyInit1, InotifyRmWatch, Ioctl,
            Ioperm, Iopl, IoprioGet, IoprioSet, Kcmp, Keyctl, Kill, Lgetxattr, Link, LinkAt,
            Listen, Listxattr, Llistxattr, Lremovexattr, Lseek, Lsetxattr, Madvise, MemfdCreate,
            Mincore, Mkdir, MkdirAt, Mknodat, Mlock, Mmap, Mount, MountSetattr, MoveMount,
            Mprotect, Mremap, Msync, Munlock, Munmap, NameToHandleAt, Nanosleep, Newfstatat, Open,
            OpenAt, OpenFlags, OpenTree, Pause, PidfdOpen, PidfdSendSignal, Pipe, Pipe2, Poll,
            PollEvents, PollTimespec, Ppoll, Prctl, Pread64, Prlimit64, Pselect6, Ptrace, Pwrite64,
            Read, Readlink, ReadlinkAt, Reboot, Recvfrom, Recvmsg, Removexattr, Rename, RenameAt,
            RenameAt2, Rmdir, Rseq, RtSigaction, RtSigpending, RtSigprocmask, RtSigqueueinfo,
            RtSigsuspend, RtSigtimedwait, SchedGetPriorityMax, SchedGetPriorityMin,
            SchedGetaffinity, SchedGetparam, SchedGetscheduler, SchedRrGetInterval,
            SchedSetaffinity, SchedSetparam, SchedSetscheduler, SchedYield, SelectTimespec,
            Sendfile, Sendmmsg, Sendmsg, Sendto, SetRobustList, SetTidAddress, Setfsgid, Setfsuid,
            Setgid, Setgroups, Sethostname, Setitimer, Setns, Setpgid, Setpriority, Setregid,
            Setresgid, Setresuid, Setreuid, Setrlimit, Setsid, Setsockopt, Settimeofday, Setuid,
            Setxattr, Shmat, Shmctl, Shmdt, Shmget, Shutdown, Sigaltstack, Signalfd4, Socket,
            Socketpair, Splice, Statfs, Statx, Symlink, SymlinkAt, Sync, Sysinfo, Tgkill, Time,
            TimerCreate, TimerDelete, TimerGetoverrun, TimerGettime, TimerSettime, TimerfdCreate,
            TimerfdGettime, TimerfdSettime, Umask, Umount2, Uname, Unlink, UnlinkAt, Unshare,
            Utimensat, Vhangup, Wait4, Waitid, Write, Writev, clear_fdset, fdset_contains,
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
    thread::THREAD_MANAGER,
};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct LinuxDirent64Header {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxStatxTimestamp {
    tv_sec: i64,
    tv_nsec: u32,
    __reserved: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxStatx {
    stx_mask: u32,
    stx_blksize: u32,
    stx_attributes: u64,
    stx_nlink: u32,
    stx_uid: u32,
    stx_gid: u32,
    stx_mode: u16,
    __spare0: u16,
    stx_ino: u64,
    stx_size: u64,
    stx_blocks: u64,
    stx_attributes_mask: u64,
    stx_atime: TestLinuxStatxTimestamp,
    stx_btime: TestLinuxStatxTimestamp,
    stx_ctime: TestLinuxStatxTimestamp,
    stx_mtime: TestLinuxStatxTimestamp,
    stx_rdev_major: u32,
    stx_rdev_minor: u32,
    stx_dev_major: u32,
    stx_dev_minor: u32,
    stx_mnt_id: u64,
    stx_dio_mem_align: u32,
    stx_dio_offset_align: u32,
    __spare3: [u64; 12],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxFileHandle {
    handle_bytes: u32,
    handle_type: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(dead_code)]
struct TestLinuxPollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct TestLinuxEpollEvent {
    events: u32,
    data: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxSignalfdSiginfo {
    ssi_signo: u32,
    ssi_errno: i32,
    ssi_code: i32,
    ssi_pid: u32,
    ssi_uid: u32,
    ssi_fd: i32,
    ssi_tid: u32,
    ssi_band: u32,
    ssi_overrun: u32,
    ssi_trapno: u32,
    ssi_status: i32,
    ssi_int: i32,
    ssi_ptr: u64,
    ssi_utime: u64,
    ssi_stime: u64,
    ssi_addr: u64,
    ssi_addr_lsb: u16,
    __pad2: u16,
    ssi_syscall: i32,
    ssi_call_addr: u64,
    ssi_arch: u32,
    __pad: [u8; 28],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TestLinuxSockAddrUn {
    sun_family: u16,
    sun_path: [u8; 108],
}

impl Default for TestLinuxSockAddrUn {
    fn default() -> Self {
        Self {
            sun_family: 0,
            sun_path: [0; 108],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct TestLinuxSockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestRelibcIovec {
    iov_base: *mut u8,
    iov_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestRelibcMsgHdr {
    msg_name: *mut u8,
    msg_namelen: u32,
    msg_iov: *mut TestRelibcIovec,
    msg_iovlen: usize,
    msg_control: *mut u8,
    msg_controllen: usize,
    msg_flags: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestRelibcMmsghdr {
    msg_hdr: TestRelibcMsgHdr,
    msg_len: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxCmsgHdr {
    cmsg_len: usize,
    cmsg_level: i32,
    cmsg_type: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxUcred {
    pid: i32,
    uid: u32,
    gid: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestRightsControlMessage {
    header: TestLinuxCmsgHdr,
    fd: i32,
    pad: i32,
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
    posix_timer_syscalls,
    "posix timer syscalls follow linux rules",
    posix_timer_syscalls_follow_linux_rules
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
    filesystem_statx_syscalls,
    "statx follows linux rules",
    filesystem_statx_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_name_to_handle_short_buffer_syscalls,
    "name_to_handle_at short buffer follows linux rules",
    filesystem_name_to_handle_short_buffer_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_name_to_handle_success_syscalls,
    "name_to_handle_at success path follows linux rules",
    filesystem_name_to_handle_success_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_name_to_handle_null_handle_syscalls,
    "name_to_handle_at null handle follows linux rules",
    filesystem_name_to_handle_null_handle_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_name_to_handle_null_mount_id_syscalls,
    "name_to_handle_at null mount id follows linux rules",
    filesystem_name_to_handle_null_mount_id_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_name_to_handle_bad_flag_syscalls,
    "name_to_handle_at invalid flag follows linux rules",
    filesystem_name_to_handle_bad_flag_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_utimensat_success_syscalls,
    "utimensat success paths follow linux rules",
    filesystem_utimensat_success_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_utimensat_negative_nsec_syscalls,
    "utimensat rejects invalid nanoseconds like linux",
    filesystem_utimensat_negative_nsec_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_utimensat_null_path_empty_path_syscalls,
    "utimensat rejects null path with empty_path like linux",
    filesystem_utimensat_null_path_empty_path_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_utimensat_empty_path_without_flag_syscalls,
    "utimensat rejects empty path without empty_path like linux",
    filesystem_utimensat_empty_path_without_flag_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_utimensat_at_fdcwd_null_path_syscalls,
    "utimensat rejects at_fdcwd with null path like linux",
    filesystem_utimensat_at_fdcwd_null_path_syscalls_follow_linux_rules
);
crate::test!(
    filesystem_utimensat_invalid_flag_syscalls,
    "utimensat rejects invalid flags like linux",
    filesystem_utimensat_invalid_flag_syscalls_follow_linux_rules
);
crate::test!(
    poll_and_ppoll_syscalls,
    "poll and ppoll follow linux rules",
    poll_and_ppoll_syscalls_follow_linux_rules
);
crate::test!(
    epoll_syscalls,
    "epoll syscalls follow linux rules",
    epoll_syscalls_follow_linux_rules
);
crate::test!(
    signalfd_syscalls,
    "signalfd syscalls follow linux rules",
    signalfd_syscalls_follow_linux_rules
);
crate::test!(
    socket_name_and_shutdown_syscalls,
    "socketpair shutdown getsockname and getpeername follow linux rules",
    socket_name_and_shutdown_syscalls_follow_linux_rules
);
crate::test!(
    socket_bind_connect_accept_syscalls,
    "bind listen connect and accept4 follow linux socket rules",
    socket_bind_connect_accept_syscalls_follow_linux_rules
);
crate::test!(
    socket_message_syscalls,
    "accept sendto recvfrom sendmsg sendmmsg and recvmsg follow linux socket rules",
    socket_message_syscalls_follow_linux_rules
);
crate::test!(
    namespace_and_kcmp_syscalls,
    "namespace and kcmp syscalls follow linux rules",
    namespace_and_kcmp_syscalls_follow_linux_rules
);
crate::test!(
    close_range_syscalls,
    "close_range follows linux fd rules",
    close_range_syscalls_follow_linux_rules
);
crate::test!(
    pidfd_and_waitid_syscalls,
    "pidfd_open and waitid follow linux process rules",
    pidfd_and_waitid_syscalls_follow_linux_rules
);
crate::test!(
    sleep_and_signal_mask_syscalls,
    "nanosleep setitimer and rt_sigsuspend follow linux rules",
    sleep_and_signal_mask_syscalls_follow_linux_rules
);
crate::test!(
    epoll_pwait2_syscalls,
    "epoll_pwait2 follows linux timeout rules",
    epoll_pwait2_syscalls_follow_linux_rules
);
crate::test!(
    object_control_syscalls,
    "ioctl and sched_setscheduler follow linux rules",
    object_control_syscalls_follow_linux_rules
);
crate::test!(
    ptrace_syscalls,
    "ptrace syscalls follow linux rules",
    ptrace_syscalls_follow_linux_rules
);
crate::test!(
    mount_api_syscalls,
    "mount and new mount api syscalls follow linux rules",
    mount_api_syscalls_follow_linux_rules
);
crate::test!(
    process_and_signal_transition_helpers,
    "signal return and process transition helpers follow linux rules",
    process_and_signal_transition_helpers_follow_linux_rules
);
crate::test!(
    clone_and_fork_syscalls,
    "clone fork and clone3 syscalls follow linux rules",
    clone_and_fork_syscalls_follow_linux_rules
);
crate::test!(
    futex_syscalls,
    "futex syscalls follow linux rules",
    futex_syscalls_follow_linux_rules
);
crate::test!(
    execve_syscalls,
    "execve syscall semantics follow linux rules",
    execve_syscalls_follow_linux_rules
);
crate::test!(
    exit_thread_semantics,
    "exit helper semantics follow linux rules",
    exit_thread_semantics_follow_linux_rules
);
crate::test!(
    exit_group_semantics,
    "exit_group helper semantics follow linux rules",
    exit_group_semantics_follow_linux_rules
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
crate::test!(
    pselect6_syscalls,
    "pselect6 follows linux rules",
    pselect6_syscalls_follow_linux_rules
);
crate::test!(
    memory_mapping_syscalls,
    "brk mmap mprotect munmap mremap msync and mincore follow linux rules",
    memory_mapping_syscalls_follow_linux_rules
);
crate::test!(
    sysv_shm_syscalls,
    "sysv shm syscalls follow linux rules",
    sysv_shm_syscalls_follow_linux_rules
);
crate::test!(
    key_and_bpf_syscalls,
    "add_key keyctl and bpf follow linux rules",
    key_and_bpf_syscalls_follow_linux_rules
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
struct TestLinuxCloneArgs {
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

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxStack {
    ss_sp: u64,
    ss_flags: i32,
    ss_size: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxSigAction {
    handler: usize,
    flags: u64,
    restorer: usize,
    mask: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxSigSetArg {
    sigmask: u64,
    sigsetsize: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestBpfMapCreateAttr {
    map_type: u32,
    key_size: u32,
    value_size: u32,
    max_entries: u32,
    map_flags: u32,
    inner_map_fd: u32,
    numa_node: u32,
    map_name: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestBpfMapElemAttr {
    map_fd: u32,
    padding: u32,
    key: u64,
    value: u64,
    flags: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestBpfProgLoadAttr {
    prog_type: u32,
    insn_cnt: u32,
    insns: u64,
    license: u64,
    log_level: u32,
    log_size: u32,
    log_buf: u64,
    kern_version: u32,
    prog_flags: u32,
    prog_name: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestBpfProgAttachAttr {
    target_fd: u32,
    attach_bpf_fd: u32,
    attach_type: u32,
    attach_flags: u32,
    replace_bpf_fd: u32,
    relative_fd: u32,
    expected_revision: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxItimerspec {
    it_interval: TestLinuxTimespec,
    it_value: TestLinuxTimespec,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxItimerval {
    it_interval: TestLinuxTimeval,
    it_value: TestLinuxTimeval,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TestWaitidSigInfo {
    si_signo: i32,
    si_errno: i32,
    si_code: i32,
    _pad0: i32,
    si_pid: i32,
    si_uid: u32,
    si_status: i32,
    _pad1: i32,
    si_utime: i64,
    si_stime: i64,
    _rest: [u8; 80],
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

    let page = allocate_user_test_page();
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
        SyscallArgs::new([signal_timer_id as u64, page + 128, 0, 0, 0, 0]).call::<TimerGettime>(),
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
        SyscallArgs::new([signal_timer_id as u64, page + 320, 0, 0, 0, 0]).call::<TimerGettime>(),
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
        SyscallArgs::new([signal_timer_id as u64, page + 448, 0, 0, 0, 0]).call::<TimerGettime>(),
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
        SyscallArgs::new([signal_timer_id as u64, 0, page + 512, 0, 0, 0]).call::<TimerSettime>(),
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
        SyscallArgs::new([default_timer_id as u64, page + 640, 0, 0, 0, 0]).call::<TimerGettime>(),
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
    const SEEK_CUR: u64 = 1;
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
    write_user_cstr(user_page + 192, b"/tmp/syscall-file-object-test/out\0");
    let copy_out_fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            user_page + 192,
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
    get_object_current_process(copy_out_fd as u64)
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

    write_user_value(user_page + 704, b"abcdef");
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
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 704, 6, 0, 0, 0]).call::<Write>(),
        6,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([copy_out_fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    assert_eq!(
        SyscallArgs::new([copy_out_fd as u64, fd as u64, 0, 3, 0, 0]).call::<Sendfile>(),
        Ok(3),
        "sendfile result",
    );
    expect_ok(
        SyscallArgs::new([copy_out_fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((user_page + 576) as *mut u8, &[0; 6])
        .unwrap();
    assert_eq!(
        SyscallArgs::new([copy_out_fd as u64, user_page + 576, 6, 0, 0, 0]).call::<Read>(),
        Ok(3),
        "sendfile readback",
    );
    assert_user_bytes(user_page + 576, b"abc");
    write_user_value(user_page + 608, &1i64);
    assert_eq!(
        SyscallArgs::new([copy_out_fd as u64, fd as u64, user_page + 608, 2, 0, 0])
            .call::<Sendfile>(),
        Ok(2),
        "sendfile offset result",
    );
    assert_eq!(read_user_value::<i64>(user_page + 608), 3);
    expect_ok(
        SyscallArgs::new([fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
        3,
    );

    get_object_current_process(fd as u64)
        .unwrap()
        .as_file_like()
        .unwrap()
        .truncate(0)
        .unwrap();
    get_object_current_process(copy_out_fd as u64)
        .unwrap()
        .as_file_like()
        .unwrap()
        .truncate(0)
        .unwrap();
    let pipe_page = user_page + 800;
    expect_ok(
        SyscallArgs::new([pipe_page, 0, 0, 0, 0, 0]).call::<Pipe>(),
        0,
    );
    let pipe_fds = read_user_value::<[i32; 2]>(pipe_page);
    let pipe_read_fd = pipe_fds[0] as usize;
    let pipe_write_fd = pipe_fds[1] as usize;
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 704, 6, 0, 0, 0]).call::<Write>(),
        6,
    );
    write_user_value(user_page + 616, &1i64);
    assert_eq!(
        SyscallArgs::new([fd as u64, user_page + 616, pipe_write_fd as u64, 0, 3, 0,])
            .call::<Splice>(),
        Ok(3),
        "splice result",
    );
    assert_eq!(read_user_value::<i64>(user_page + 616), 4);
    expect_ok(
        SyscallArgs::new([fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
        6,
    );
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((user_page + 632) as *mut u8, &[0; 6])
        .unwrap();
    assert_eq!(
        SyscallArgs::new([pipe_read_fd as u64, user_page + 632, 6, 0, 0, 0]).call::<Read>(),
        Ok(3),
        "splice readback",
    );
    assert_user_bytes(user_page + 632, b"bcd");
    expect_errno(
        SyscallArgs::new([fd as u64, user_page + 616, pipe_write_fd as u64, 0, 1, 1])
            .call::<Splice>(),
        SyscallError::InvalidArguments,
    );

    get_object_current_process(fd as u64)
        .unwrap()
        .as_file_like()
        .unwrap()
        .truncate(0)
        .unwrap();
    get_object_current_process(copy_out_fd as u64)
        .unwrap()
        .as_file_like()
        .unwrap()
        .truncate(0)
        .unwrap();
    expect_ok(
        SyscallArgs::new([fd as u64, user_page + 704, 6, 0, 0, 0]).call::<Write>(),
        6,
    );
    write_user_value(user_page + 640, &2i64);
    write_user_value(user_page + 648, &1i64);
    assert_eq!(
        SyscallArgs::new([
            fd as u64,
            user_page + 640,
            copy_out_fd as u64,
            user_page + 648,
            2,
            0,
        ])
        .call::<CopyFileRange>(),
        Ok(2),
        "copy_file_range result",
    );
    assert_eq!(read_user_value::<i64>(user_page + 640), 4);
    assert_eq!(read_user_value::<i64>(user_page + 648), 3);
    expect_ok(
        SyscallArgs::new([copy_out_fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((user_page + 656) as *mut u8, &[0; 6])
        .unwrap();
    assert_eq!(
        SyscallArgs::new([copy_out_fd as u64, user_page + 656, 6, 0, 0, 0]).call::<Read>(),
        Ok(3),
        "copy_file_range readback",
    );
    assert_user_bytes(user_page + 656, b"\0cd");
    expect_ok(
        SyscallArgs::new([fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
        6,
    );
    expect_ok(
        SyscallArgs::new([copy_out_fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    write_user_value(user_page + 640, &0i64);
    assert_eq!(
        SyscallArgs::new([fd as u64, user_page + 640, copy_out_fd as u64, 0, 1, 0])
            .call::<CopyFileRange>(),
        Ok(1),
        "copy_file_range mixed offset result",
    );
    assert_eq!(read_user_value::<i64>(user_page + 640), 1);
    expect_ok(
        SyscallArgs::new([fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
        6,
    );
    expect_ok(
        SyscallArgs::new([copy_out_fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
        1,
    );
    expect_errno(
        SyscallArgs::new([fd as u64, 0, copy_out_fd as u64, 0, 1, 1]).call::<CopyFileRange>(),
        SyscallError::InvalidArguments,
    );

    close_test_fd(pipe_write_fd);
    close_test_fd(pipe_read_fd);
    close_test_fd(copy_out_fd);
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

fn filesystem_statx_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const AT_EMPTY_PATH: u64 = 0x1000;
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
    const AT_STATX_FORCE_SYNC: u64 = 0x2000;
    const AT_STATX_DONT_SYNC: u64 = 0x4000;
    const STATX_BASIC_STATS: u64 = 0x0000_07ff;
    const STATX_MNT_ID: u32 = 0x0000_1000;
    const STATX_ATTR_MOUNT_ROOT: u64 = 0x0000_2000;

    let base_path = Path::new("/tmp/syscall-statx-test");
    let cleanup_paths = [
        "/tmp/syscall-statx-test/link",
        "/tmp/syscall-statx-test/file",
        "/tmp/syscall-statx-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-statx-test/file"))
        .unwrap();
    VirtualFS
        .lock()
        .create_symlink(
            Path::new("/tmp/syscall-statx-test/link"),
            "/tmp/syscall-statx-test/file",
        )
        .unwrap();

    assert_linux_layout::<TestLinuxStatxTimestamp>(16, 8);
    assert_linux_layout::<TestLinuxStatx>(256, 8);
    assert_linux_layout::<TestLinuxFileHandle>(8, 4);

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-statx-test/file\0");
    write_user_cstr(user_page + 64, b"/tmp/syscall-statx-test/link\0");
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
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            0,
            STATX_BASIC_STATS,
            user_page + 256,
            0,
        ])
        .call::<Statx>(),
        0,
    );
    let statx = read_user_value::<TestLinuxStatx>(user_page + 256);
    let file_stat = {
        let file = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-statx-test/file")).unwrap()
        };
        file.stat()
    };
    assert_eq!(statx.stx_mask, STATX_BASIC_STATS as u32 | STATX_MNT_ID);
    assert_eq!(statx.stx_mode, file_stat.st_mode as u16);
    assert_eq!(statx.stx_nlink, file_stat.st_nlink as u32);
    assert_eq!(statx.stx_size, file_stat.st_size as u64);
    assert_eq!(statx.stx_ino, file_stat.st_ino);
    assert_eq!(statx.stx_attributes_mask, STATX_ATTR_MOUNT_ROOT);
    assert_eq!(statx.stx_attributes & STATX_ATTR_MOUNT_ROOT, 0);
    assert!(statx.stx_mnt_id >= 1);
    assert_eq!(statx.stx_btime.tv_sec, 0);
    assert_eq!(statx.stx_btime.tv_nsec, 0);

    expect_ok(
        SyscallArgs::new([
            AT_FDCWD,
            user_page + 64,
            AT_SYMLINK_NOFOLLOW,
            STATX_BASIC_STATS,
            user_page + 256,
            0,
        ])
        .call::<Statx>(),
        0,
    );
    let link_statx = read_user_value::<TestLinuxStatx>(user_page + 256);
    assert_ne!(link_statx.stx_ino, statx.stx_ino);

    expect_ok(
        SyscallArgs::new([
            file_fd as u64,
            0,
            AT_EMPTY_PATH,
            STATX_BASIC_STATS,
            user_page + 256,
            0,
        ])
        .call::<Statx>(),
        0,
    );
    let empty_path_statx = read_user_value::<TestLinuxStatx>(user_page + 256);
    assert_eq!(empty_path_statx.stx_ino, statx.stx_ino);

    expect_errno(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            0x8000_0000,
            STATX_BASIC_STATS,
            user_page + 256,
            0,
        ])
        .call::<Statx>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            AT_STATX_FORCE_SYNC | AT_STATX_DONT_SYNC,
            STATX_BASIC_STATS,
            user_page + 256,
            0,
        ])
        .call::<Statx>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([file_fd as u64, 0, 0, STATX_BASIC_STATS, user_page + 256, 0])
            .call::<Statx>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([file_fd as u64, 0, AT_EMPTY_PATH, STATX_BASIC_STATS, 0, 0])
            .call::<Statx>(),
        SyscallError::BadAddress,
    );

    close_test_fd(file_fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_name_to_handle_short_buffer_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let base_path = Path::new("/tmp/syscall-name-handle-test");
    let cleanup_paths = [
        "/tmp/syscall-name-handle-test/file",
        "/tmp/syscall-name-handle-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
        .unwrap();

    assert_linux_layout::<TestLinuxFileHandle>(8, 4);

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
    write_user_value(
        user_page + 512,
        &TestLinuxFileHandle {
            handle_bytes: 4,
            handle_type: 0,
        },
    );
    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, user_page + 512, user_page + 520, 0, 0])
            .call::<NameToHandleAt>(),
        SyscallError::ValueTooLarge,
    );
    let short_handle = read_user_value::<TestLinuxFileHandle>(user_page + 512);
    assert_eq!(short_handle.handle_bytes, 8);
    assert_eq!(short_handle.handle_type, 1);
    assert!(read_user_value::<i32>(user_page + 520) >= 1);

    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_name_to_handle_success_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let base_path = Path::new("/tmp/syscall-name-handle-test");
    let cleanup_paths = [
        "/tmp/syscall-name-handle-test/file",
        "/tmp/syscall-name-handle-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
        .unwrap();

    assert_linux_layout::<TestLinuxFileHandle>(8, 4);

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
    let file_stat = {
        let file = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-name-handle-test/file"))
                .unwrap()
        };
        file.stat()
    };

    write_user_value(
        user_page + 512,
        &TestLinuxFileHandle {
            handle_bytes: 8,
            handle_type: 0,
        },
    );
    expect_ok(
        SyscallArgs::new([AT_FDCWD, user_page, user_page + 512, user_page + 520, 0, 0])
            .call::<NameToHandleAt>(),
        0,
    );
    let full_handle = read_user_value::<TestLinuxFileHandle>(user_page + 512);
    assert_eq!(full_handle.handle_bytes, 8);
    assert_eq!(full_handle.handle_type, 1);
    assert_eq!(
        read_user_value::<u64>(user_page + 512 + 8),
        file_stat.st_ino
    );

    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_name_to_handle_null_handle_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let base_path = Path::new("/tmp/syscall-name-handle-test");
    let cleanup_paths = [
        "/tmp/syscall-name-handle-test/file",
        "/tmp/syscall-name-handle-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
    write_user_value(
        user_page + 512,
        &TestLinuxFileHandle {
            handle_bytes: 8,
            handle_type: 0,
        },
    );

    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, 0, user_page + 520, 0, 0]).call::<NameToHandleAt>(),
        SyscallError::BadAddress,
    );

    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_name_to_handle_null_mount_id_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let base_path = Path::new("/tmp/syscall-name-handle-test");
    let cleanup_paths = [
        "/tmp/syscall-name-handle-test/file",
        "/tmp/syscall-name-handle-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
    write_user_value(
        user_page + 512,
        &TestLinuxFileHandle {
            handle_bytes: 8,
            handle_type: 0,
        },
    );

    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, user_page + 512, 0, 0, 0]).call::<NameToHandleAt>(),
        SyscallError::BadAddress,
    );

    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_name_to_handle_bad_flag_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let base_path = Path::new("/tmp/syscall-name-handle-test");
    let cleanup_paths = [
        "/tmp/syscall-name-handle-test/file",
        "/tmp/syscall-name-handle-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
    write_user_value(
        user_page + 512,
        &TestLinuxFileHandle {
            handle_bytes: 8,
            handle_type: 0,
        },
    );

    expect_errno(
        SyscallArgs::new([
            AT_FDCWD,
            user_page,
            user_page + 512,
            user_page + 520,
            0x4000_0000,
            0,
        ])
        .call::<NameToHandleAt>(),
        SyscallError::InvalidArguments,
    );

    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_utimensat_success_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const AT_EMPTY_PATH: u64 = 0x1000;
    const UTIME_OMIT: i64 = 0x3fff_ffff;

    let base_path = Path::new("/tmp/syscall-utimensat-test");
    let cleanup_paths = [
        "/tmp/syscall-utimensat-test/file",
        "/tmp/syscall-utimensat-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-utimensat-test/file"))
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-utimensat-test/file\0");
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

    let valid_times = [[0i64, 0i64], [0i64, UTIME_OMIT]];
    write_user_value(user_page + 640, &valid_times);
    expect_ok(
        SyscallArgs::new([file_fd as u64, 0, user_page + 640, 0, 0, 0]).call::<Utimensat>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([file_fd as u64, user_page, user_page + 640, 0, 0, 0]).call::<Utimensat>(),
        0,
    );
    write_user_cstr(user_page + 704, b"\0");
    expect_ok(
        SyscallArgs::new([
            file_fd as u64,
            user_page + 704,
            user_page + 640,
            AT_EMPTY_PATH,
            0,
            0,
        ])
        .call::<Utimensat>(),
        0,
    );

    close_test_fd(file_fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn prepare_utimensat_test_file() -> (usize, [u64; 2]) {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let base_path = Path::new("/tmp/syscall-utimensat-test");
    let cleanup_paths = [
        "/tmp/syscall-utimensat-test/file",
        "/tmp/syscall-utimensat-test",
    ];
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-utimensat-test/file"))
        .unwrap();

    let user_page = allocate_user_test_page();
    write_user_cstr(user_page, b"/tmp/syscall-utimensat-test/file\0");
    write_user_cstr(user_page + 704, b"\0");
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

    (file_fd, [user_page, user_page + 640])
}

fn cleanup_utimensat_test_file(file_fd: usize) {
    let cleanup_paths = [
        "/tmp/syscall-utimensat-test/file",
        "/tmp/syscall-utimensat-test",
    ];
    close_test_fd(file_fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

fn filesystem_utimensat_negative_nsec_syscalls_follow_linux_rules() {
    let (file_fd, pages) = prepare_utimensat_test_file();
    let [user_page, times_page] = pages;

    write_user_value(times_page, &[[0i64, 0i64], [0i64, -1i64]]);
    expect_errno(
        SyscallArgs::new([file_fd as u64, user_page, times_page, 0, 0, 0]).call::<Utimensat>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(times_page, &[[-1i64, -1i64], [0i64, 0i64]]);
    expect_errno(
        SyscallArgs::new([file_fd as u64, user_page, times_page, 0, 0, 0]).call::<Utimensat>(),
        SyscallError::InvalidArguments,
    );

    cleanup_utimensat_test_file(file_fd);
}

fn filesystem_utimensat_null_path_empty_path_syscalls_follow_linux_rules() {
    const AT_EMPTY_PATH: u64 = 0x1000;

    let (file_fd, [_user_page, times_page]) = prepare_utimensat_test_file();
    write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
    expect_errno(
        SyscallArgs::new([file_fd as u64, 0, times_page, AT_EMPTY_PATH, 0, 0]).call::<Utimensat>(),
        SyscallError::InvalidArguments,
    );

    cleanup_utimensat_test_file(file_fd);
}

fn filesystem_utimensat_empty_path_without_flag_syscalls_follow_linux_rules() {
    let (file_fd, pages) = prepare_utimensat_test_file();
    let [_user_page, times_page] = pages;

    write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
    expect_errno(
        SyscallArgs::new([file_fd as u64, times_page + 64, times_page, 0, 0, 0])
            .call::<Utimensat>(),
        SyscallError::FileNotFound,
    );

    cleanup_utimensat_test_file(file_fd);
}

fn filesystem_utimensat_at_fdcwd_null_path_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let (file_fd, [_user_page, times_page]) = prepare_utimensat_test_file();
    write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
    expect_errno(
        SyscallArgs::new([AT_FDCWD, 0, times_page, 0, 0, 0]).call::<Utimensat>(),
        SyscallError::BadAddress,
    );

    cleanup_utimensat_test_file(file_fd);
}

fn filesystem_utimensat_invalid_flag_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let (file_fd, [user_page, times_page]) = prepare_utimensat_test_file();
    write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, times_page, 0x200, 0, 0]).call::<Utimensat>(),
        SyscallError::InvalidArguments,
    );

    cleanup_utimensat_test_file(file_fd);
}

#[allow(dead_code)]
fn poll_and_ppoll_syscalls_follow_linux_rules() {
    const POLLIN: i16 = 0x001;
    const POLLOUT: i16 = 0x004;
    const POLLNVAL: i16 = 0x020;

    assert_linux_layout::<TestLinuxPollFd>(8, 4);

    let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let poll_page = allocate_user_test_page();
    write_user_value(
        poll_page,
        &[
            TestLinuxPollFd {
                fd: eventfd as i32,
                events: POLLOUT,
                revents: 0,
            },
            TestLinuxPollFd {
                fd: 4096,
                events: POLLIN,
                revents: 0,
            },
        ],
    );
    expect_ok(
        SyscallArgs::new([poll_page, 2, 0, 0, 0, 0]).call::<Poll>(),
        2,
    );
    let pollfds = read_user_value::<[TestLinuxPollFd; 2]>(poll_page);
    assert_eq!(pollfds[0].revents & POLLOUT, POLLOUT);
    assert_eq!(pollfds[1].revents & POLLNVAL, POLLNVAL);

    write_user_value(
        poll_page,
        &[TestLinuxPollFd {
            fd: eventfd as i32,
            events: POLLIN,
            revents: 123,
        }],
    );
    expect_ok(
        SyscallArgs::new([poll_page, 1, 0, 0, 0, 0]).call::<Poll>(),
        0,
    );
    assert_eq!(read_user_value::<TestLinuxPollFd>(poll_page).revents, 0);

    let ppoll_timeout = TestLinuxTimespec {
        tv_sec: 0,
        tv_nsec: 1_000_000,
    };
    write_user_value(
        poll_page,
        &[TestLinuxPollFd {
            fd: eventfd as i32,
            events: POLLOUT,
            revents: 0,
        }],
    );
    write_user_value(poll_page + 128, &ppoll_timeout);
    let ppoll_result = SyscallArgs::new([poll_page, 1, poll_page + 128, 0, 0, 0]).call::<Ppoll>();
    expect_ok(ppoll_result, 1);
    assert_eq!(
        read_user_value::<TestLinuxPollFd>(poll_page).revents & POLLOUT,
        POLLOUT
    );
    let sigmask: u64 = Signal::SIGUSR1.mask();
    write_user_value(poll_page + 192, &sigmask);
    expect_errno(
        SyscallArgs::new([poll_page, 1, poll_page + 128, poll_page + 192, 4, 0]).call::<Ppoll>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(
        poll_page + 128,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        },
    );
    expect_errno(
        SyscallArgs::new([poll_page, 1, poll_page + 128, 0, 0, 0]).call::<Ppoll>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Poll>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Ppoll>(),
        SyscallError::BadAddress,
    );

    close_test_fd(eventfd);
}

fn epoll_syscalls_follow_linux_rules() {
    const EPOLL_CTL_ADD: u64 = 1;
    const EPOLL_CTL_MOD: u64 = 3;
    const EPOLL_CTL_DEL: u64 = 2;
    const EPOLLIN: u32 = 0x001;
    const EPOLLOUT: u32 = 0x004;
    const EPOLLHUP: u32 = 0x010;
    const EPOLLRDHUP: u32 = 0x2000;
    const EPOLLONESHOT: u32 = 0x4000_0000;
    const AF_UNIX: u64 = 1;
    const SOCK_STREAM: u64 = 1;
    assert_linux_layout::<TestLinuxEpollEvent>(12, 1);

    let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let epoll_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<EpollCreate1>());
    let event = TestLinuxEpollEvent {
        events: EPOLLOUT,
        data: 0xfeed_beef,
    };
    expect_ok(
        SyscallArgs::new([
            epoll_fd as u64,
            EPOLL_CTL_ADD,
            eventfd as u64,
            (&event as *const TestLinuxEpollEvent) as u64,
            0,
            0,
        ])
        .call::<EpollCtl>(),
        0,
    );
    let epoll_events = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([epoll_fd as u64, epoll_events, 4, 0, 0, 0]).call::<EpollWait>(),
        1,
    );
    let ready = read_user_value::<TestLinuxEpollEvent>(epoll_events);
    let ready_events = ready.events;
    let ready_data = ready.data;
    assert_eq!(ready_events, EPOLLOUT);
    assert_eq!(ready_data, 0xfeed_beef);

    let oneshot = TestLinuxEpollEvent {
        events: EPOLLOUT | EPOLLONESHOT,
        data: 7,
    };
    expect_ok(
        SyscallArgs::new([
            epoll_fd as u64,
            EPOLL_CTL_MOD,
            eventfd as u64,
            (&oneshot as *const TestLinuxEpollEvent) as u64,
            0,
            0,
        ])
        .call::<EpollCtl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([epoll_fd as u64, epoll_events, 4, 0, 0, 0]).call::<EpollPwait>(),
        1,
    );
    expect_ok(
        SyscallArgs::new([epoll_fd as u64, epoll_events, 4, 0, 0, 0]).call::<EpollWait>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_DEL, eventfd as u64, 0, 0, 0])
            .call::<EpollCtl>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([epoll_fd as u64, 99, eventfd as u64, 0, 0, 0]).call::<EpollCtl>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_ADD, eventfd as u64, 0, 0, 0])
            .call::<EpollCtl>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([epoll_fd as u64, epoll_events, 0, 0, 0, 0]).call::<EpollWait>(),
        SyscallError::InvalidArguments,
    );

    let socketpair_page = epoll_events + 128;
    expect_ok(
        SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, socketpair_page, 0, 0]).call::<Socketpair>(),
        0,
    );
    let left = read_user_value::<i32>(socketpair_page) as usize;
    let right = read_user_value::<i32>(socketpair_page + 4) as usize;
    let socket_event = TestLinuxEpollEvent {
        events: EPOLLIN | EPOLLOUT | EPOLLHUP | EPOLLRDHUP,
        data: 0x55aa,
    };
    write_user_value(epoll_events + 192, &socket_event);
    expect_ok(
        SyscallArgs::new([
            epoll_fd as u64,
            EPOLL_CTL_ADD,
            left as u64,
            epoll_events + 192,
            0,
            0,
        ])
        .call::<EpollCtl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([epoll_fd as u64, epoll_events + 256, 4, 0, 0, 0]).call::<EpollWait>(),
        1,
    );
    let socket_ready = read_user_value::<TestLinuxEpollEvent>(epoll_events + 256);
    let socket_ready_events = socket_ready.events;
    let socket_ready_data = socket_ready.data;
    assert_eq!(socket_ready_events, EPOLLOUT);
    assert_eq!(socket_ready_data, 0x55aa);

    write_user_value(epoll_events + 320, b"u");
    expect_ok(
        SyscallArgs::new([right as u64, epoll_events + 320, 1, 0, 0, 0]).call::<Write>(),
        1,
    );
    expect_ok(
        SyscallArgs::new([epoll_fd as u64, epoll_events + 384, 4, 0, 0, 0]).call::<EpollWait>(),
        1,
    );
    let readable_socket_ready = read_user_value::<TestLinuxEpollEvent>(epoll_events + 384);
    let readable_socket_events = readable_socket_ready.events;
    let readable_socket_data = readable_socket_ready.data;
    assert_eq!(readable_socket_events, EPOLLIN | EPOLLOUT);
    assert_eq!(readable_socket_data, 0x55aa);
    expect_ok(
        SyscallArgs::new([left as u64, epoll_events + 321, 1, 0, 0, 0]).call::<Read>(),
        1,
    );
    assert_user_bytes(epoll_events + 321, b"u");

    expect_ok(
        SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_DEL, left as u64, 0, 0, 0]).call::<EpollCtl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([
            epoll_fd as u64,
            EPOLL_CTL_ADD,
            left as u64,
            epoll_events + 192,
            0,
            0,
        ])
        .call::<EpollCtl>(),
        0,
    );
    close_test_fd(right);
    expect_ok(
        SyscallArgs::new([epoll_fd as u64, epoll_events + 512, 4, 0, 0, 0]).call::<EpollWait>(),
        1,
    );
    let peer_closed_socket_ready = read_user_value::<TestLinuxEpollEvent>(epoll_events + 512);
    let peer_closed_socket_events = peer_closed_socket_ready.events;
    let peer_closed_socket_data = peer_closed_socket_ready.data;
    assert_eq!(
        peer_closed_socket_events,
        EPOLLIN | EPOLLOUT | EPOLLHUP | EPOLLRDHUP
    );
    assert_eq!(peer_closed_socket_data, 0x55aa);

    expect_ok(
        SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_DEL, left as u64, 0, 0, 0]).call::<EpollCtl>(),
        0,
    );
    close_test_fd(left);
    close_test_fd(epoll_fd);
    close_test_fd(eventfd);
}

fn signalfd_syscalls_follow_linux_rules() {
    const SFD_NONBLOCK: u64 = 0o4_000;
    const SFD_CLOEXEC: u64 = 0o2_000_000;

    assert_linux_layout::<TestLinuxSignalfdSiginfo>(128, 8);

    let sigmask_user = allocate_user_test_page();
    write_user_value(sigmask_user, &Signal::SIGUSR1.mask());
    let signalfd = expect_fd(
        SyscallArgs::new([
            (-1i32) as u64,
            sigmask_user,
            core::mem::size_of::<u64>() as u64,
            SFD_NONBLOCK | SFD_CLOEXEC,
            0,
            0,
        ])
        .call::<Signalfd4>(),
    );
    assert_fd_flags(signalfd, FdFlags::CLOEXEC);
    assert_object_flags(signalfd, FileFlags::NONBLOCK);
    let siginfo_buf = allocate_user_test_page();
    expect_errno(
        SyscallArgs::new([(-1i32) as u64, sigmask_user, 4, 0, 0, 0]).call::<Signalfd4>(),
        SyscallError::InvalidArguments,
    );

    let mut siginfo: SigInfo = unsafe { core::mem::zeroed() };
    siginfo.si_signo = Signal::SIGUSR1 as i32;
    siginfo.si_errno = 123;
    siginfo.si_code = -6;
    let process = get_current_process();
    send_signal_to_process_with_siginfo(&process, Signal::SIGUSR1, siginfo);
    expect_ok(
        SyscallArgs::new([signalfd as u64, siginfo_buf, 128, 0, 0, 0]).call::<Read>(),
        128,
    );
    let signalfd_info = read_user_value::<TestLinuxSignalfdSiginfo>(siginfo_buf);
    assert_eq!(signalfd_info.ssi_signo, Signal::SIGUSR1 as u32);
    assert_eq!(signalfd_info.ssi_errno, 123);
    assert_eq!(signalfd_info.ssi_code, -6);
    assert_eq!(signalfd_info.ssi_pid, process.lock().pid.0 as u32);

    write_user_value(sigmask_user, &Signal::SIGTERM.mask());
    expect_ok(
        SyscallArgs::new([
            signalfd as u64,
            sigmask_user,
            core::mem::size_of::<u64>() as u64,
            0,
            0,
            0,
        ])
        .call::<Signalfd4>(),
        signalfd,
    );
    expect_errno(
        SyscallArgs::new([signalfd as u64, siginfo_buf, 127, 0, 0, 0]).call::<Read>(),
        SyscallError::InvalidArguments,
    );

    close_test_fd(signalfd);
}

fn socket_name_and_shutdown_syscalls_follow_linux_rules() {
    const AF_INET: u64 = 2;
    const AF_NETLINK: u64 = 16;
    const AF_UNIX: u64 = 1;
    const SOL_SOCKET: u64 = 1;
    const SOL_TCP: u64 = 6;
    const SOCK_STREAM: u64 = 1;
    const SOCK_DGRAM: u64 = 2;
    const SOCK_RAW: u64 = 3;
    const SOCK_NONBLOCK: u64 = 0o0004000;
    const SOCK_CLOEXEC: u64 = 0o2000000;
    const SHUT_RD: u64 = 0;
    const SHUT_WR: u64 = 1;
    const SHUT_RDWR: u64 = 2;
    const POLLIN: i16 = 0x001;
    const POLLOUT: i16 = 0x004;
    const POLLHUP: i16 = 0x010;
    const SO_TYPE: u64 = 3;
    const SO_ERROR: u64 = 4;
    const SO_SNDBUF: u64 = 7;
    const SO_PASSCRED: u64 = 16;
    const SO_PEERCRED: u64 = 17;
    const SO_ACCEPTCONN: u64 = 30;
    const SO_PROTOCOL: u64 = 38;
    const SO_DOMAIN: u64 = 39;
    const SO_PEERPIDFD: u64 = 77;
    const TCP_NODELAY: u64 = 1;

    assert_linux_layout::<TestLinuxSockAddrUn>(110, 2);
    assert_linux_layout::<TestLinuxSockAddrIn>(16, 2);

    let page = allocate_user_test_page();

    let socketpair_fds_page = page;
    expect_ok(
        SyscallArgs::new([
            AF_UNIX,
            SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
            0,
            socketpair_fds_page,
            0,
            0,
        ])
        .call::<Socketpair>(),
        0,
    );
    let [left_fd, right_fd] = read_user_value::<[i32; 2]>(socketpair_fds_page);
    let left_fd = usize::try_from(left_fd).expect("socketpair left fd should be non-negative");
    let right_fd = usize::try_from(right_fd).expect("socketpair right fd should be non-negative");
    assert_fd_flags(left_fd, FdFlags::CLOEXEC);
    assert_fd_flags(right_fd, FdFlags::CLOEXEC);
    assert_object_flags(left_fd, FileFlags::NONBLOCK);
    assert_object_flags(right_fd, FileFlags::NONBLOCK);

    let pollfds_page = page + 48;
    write_user_value(
        pollfds_page,
        &[TestLinuxPollFd {
            fd: left_fd as i32,
            events: POLLIN | POLLOUT | POLLHUP,
            revents: -1,
        }],
    );
    expect_ok(
        SyscallArgs::new([pollfds_page, 1, 0, 0, 0, 0]).call::<Poll>(),
        1,
    );
    let initial_poll = read_user_value::<TestLinuxPollFd>(pollfds_page);
    assert_eq!(initial_poll.revents & POLLOUT, POLLOUT);
    assert_eq!(initial_poll.revents & POLLIN, 0);
    assert_eq!(initial_poll.revents & POLLHUP, 0);

    write_user_value(page + 56, b"z");
    expect_ok(
        SyscallArgs::new([right_fd as u64, page + 56, 1, 0, 0, 0]).call::<Write>(),
        1,
    );
    write_user_value(
        pollfds_page,
        &[TestLinuxPollFd {
            fd: left_fd as i32,
            events: POLLIN | POLLOUT | POLLHUP,
            revents: 0,
        }],
    );
    expect_ok(
        SyscallArgs::new([pollfds_page, 1, 0, 0, 0, 0]).call::<Poll>(),
        1,
    );
    let readable_poll = read_user_value::<TestLinuxPollFd>(pollfds_page);
    assert_eq!(readable_poll.revents & POLLIN, POLLIN);
    assert_eq!(readable_poll.revents & POLLOUT, POLLOUT);
    assert_eq!(readable_poll.revents & POLLHUP, 0);
    expect_ok(
        SyscallArgs::new([left_fd as u64, page + 57, 1, 0, 0, 0]).call::<Read>(),
        1,
    );
    assert_user_bytes(page + 57, b"z");

    write_user_value(page + 64, &4u32);
    expect_ok(
        SyscallArgs::new([
            left_fd as u64,
            SOL_SOCKET,
            SO_PEERPIDFD,
            page + 72,
            page + 64,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(page + 64), 4);
    let socketpair_peer_pidfd = read_user_value::<i32>(page + 72);
    let socketpair_peer_pidfd =
        usize::try_from(socketpair_peer_pidfd).expect("peer pidfd should be non-negative");
    assert_fd_flags(socketpair_peer_pidfd, FdFlags::CLOEXEC);
    let current_pid = get_current_process().lock().pid.0;
    let socketpair_peer_pidfd_object = get_object_current_process(socketpair_peer_pidfd as u64)
        .expect("peer pidfd should resolve")
        .as_pidfd()
        .expect("SO_PEERPIDFD should install a pidfd");
    assert_eq!(socketpair_peer_pidfd_object.pid(), current_pid);
    write_user_value(
        page + 96,
        &[TestLinuxPollFd {
            fd: socketpair_peer_pidfd as i32,
            events: POLLIN,
            revents: -1,
        }],
    );
    expect_ok(
        SyscallArgs::new([page + 96, 1, 0, 0, 0, 0]).call::<Poll>(),
        0,
    );
    assert_eq!(read_user_value::<TestLinuxPollFd>(page + 96).revents, 0);
    close_test_fd(socketpair_peer_pidfd);

    write_user_value(page + 64, &111u32);
    expect_ok(
        SyscallArgs::new([left_fd as u64, page + 128, page + 64, 0, 0, 0]).call::<Getsockname>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(page + 64), 2);
    let local_un = read_user_value::<TestLinuxSockAddrUn>(page + 128);
    assert_eq!(local_un.sun_family, AF_UNIX as u16);
    assert!(local_un.sun_path.iter().all(|&byte| byte == 0));

    write_user_value(page + 80, &111u32);
    expect_ok(
        SyscallArgs::new([left_fd as u64, page + 256, page + 80, 0, 0, 0]).call::<Getpeername>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(page + 80), 2);
    let peer_un = read_user_value::<TestLinuxSockAddrUn>(page + 256);
    assert_eq!(peer_un.sun_family, AF_UNIX as u16);
    assert!(peer_un.sun_path.iter().all(|&byte| byte == 0));

    write_user_value(page + 96, &1u32);
    expect_ok(
        SyscallArgs::new([left_fd as u64, page + 384, page + 96, 0, 0, 0]).call::<Getpeername>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(page + 96), 2);
    assert_user_bytes(page + 384, &[AF_UNIX as u8]);

    expect_errno(
        SyscallArgs::new([left_fd as u64, page + 384, 0, 0, 0, 0]).call::<Getsockname>(),
        SyscallError::BadAddress,
    );
    write_user_value(page + 96, &4u32);
    expect_errno(
        SyscallArgs::new([left_fd as u64, 0, page + 96, 0, 0, 0]).call::<Getsockname>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([left_fd as u64, page + 384, page + 96, 99, 0, 0]).call::<Shutdown>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([left_fd as u64, SHUT_RD, 0, 0, 0, 0]).call::<Shutdown>(),
        0,
    );
    write_user_value(page + 512, b"x");
    expect_errno(
        SyscallArgs::new([right_fd as u64, page + 512, 1, 0, 0, 0]).call::<Write>(),
        SyscallError::BrokenPipe,
    );
    expect_ok(
        SyscallArgs::new([right_fd as u64, SHUT_WR, 0, 0, 0, 0]).call::<Shutdown>(),
        0,
    );
    write_user_value(
        pollfds_page,
        &[TestLinuxPollFd {
            fd: left_fd as i32,
            events: POLLIN | POLLOUT | POLLHUP,
            revents: 0,
        }],
    );
    expect_ok(
        SyscallArgs::new([pollfds_page, 1, 0, 0, 0, 0]).call::<Poll>(),
        1,
    );
    let peer_shutdown_poll = read_user_value::<TestLinuxPollFd>(pollfds_page);
    assert_eq!(peer_shutdown_poll.revents & POLLIN, POLLIN);
    assert_eq!(peer_shutdown_poll.revents & POLLOUT, POLLOUT);
    assert_eq!(peer_shutdown_poll.revents & POLLHUP, POLLHUP);
    expect_ok(
        SyscallArgs::new([left_fd as u64, page + 58, 1, 0, 0, 0]).call::<Read>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([right_fd as u64, SHUT_RDWR, 0, 0, 0, 0]).call::<Shutdown>(),
        0,
    );

    expect_errno(
        SyscallArgs::new([AF_INET, SOCK_STREAM, 0, socketpair_fds_page, 0, 0]).call::<Socketpair>(),
        SyscallError::AddressFamilyNotSupported,
    );
    expect_errno(
        SyscallArgs::new([AF_UNIX, SOCK_STREAM, 1, socketpair_fds_page, 0, 0]).call::<Socketpair>(),
        SyscallError::ProtocolNotSupported,
    );
    expect_errno(
        SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socketpair>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([AF_UNIX, 7, 0, socketpair_fds_page, 0, 0]).call::<Socketpair>(),
        SyscallError::ProtocolNotSupported,
    );

    let unix_socket = expect_fd(
        SyscallArgs::new([
            AF_UNIX,
            SOCK_DGRAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
            0,
            0,
            0,
            0,
        ])
        .call::<Socket>(),
    );
    assert_fd_flags(unix_socket, FdFlags::CLOEXEC);
    assert_object_flags(unix_socket, FileFlags::NONBLOCK);
    write_user_value(page + 896, &1i32);
    expect_ok(
        SyscallArgs::new([
            unix_socket as u64,
            SOL_SOCKET,
            SO_PASSCRED,
            page + 896,
            4,
            0,
        ])
        .call::<Setsockopt>(),
        0,
    );
    write_user_value(page + 904, &4u32);
    expect_ok(
        SyscallArgs::new([
            unix_socket as u64,
            SOL_SOCKET,
            SO_PASSCRED,
            page + 912,
            page + 904,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(page + 904), 4);
    assert_eq!(read_user_value::<i32>(page + 912), 1);
    write_user_value(page + 920, &4u32);
    expect_ok(
        SyscallArgs::new([
            unix_socket as u64,
            SOL_SOCKET,
            SO_TYPE,
            page + 928,
            page + 920,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(page + 920), 4);
    assert_eq!(read_user_value::<i32>(page + 928), SOCK_DGRAM as i32);
    write_user_value(page + 936, &4u32);
    expect_ok(
        SyscallArgs::new([
            unix_socket as u64,
            SOL_SOCKET,
            SO_DOMAIN,
            page + 944,
            page + 936,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 944), AF_UNIX as i32);
    write_user_value(page + 952, &12u32);
    expect_ok(
        SyscallArgs::new([
            unix_socket as u64,
            SOL_SOCKET,
            SO_PEERCRED,
            page + 960,
            page + 952,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(page + 952), 12);
    let peercred_words = read_user_value::<[u32; 3]>(page + 960);
    let current = get_current_process();
    let current_locked = current.lock();
    assert_eq!(peercred_words[0], current_locked.pid.0 as u32);
    assert_eq!(peercred_words[1], current_locked.effective_uid);
    assert_eq!(peercred_words[2], current_locked.effective_gid);
    drop(current_locked);
    expect_errno(
        SyscallArgs::new([
            unix_socket as u64,
            SOL_SOCKET,
            SO_PEERCRED,
            page + 960,
            0,
            0,
        ])
        .call::<Getsockopt>(),
        SyscallError::BadAddress,
    );
    write_user_value(page + 952, &3u32);
    expect_errno(
        SyscallArgs::new([
            unix_socket as u64,
            SOL_SOCKET,
            SO_TYPE,
            page + 928,
            page + 952,
            0,
        ])
        .call::<Getsockopt>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([unix_socket as u64, SOL_SOCKET, SO_TYPE, 0, page + 920, 0])
            .call::<Getsockopt>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([unix_socket as u64, SOL_SOCKET, SO_TYPE, page + 928, 0, 0])
            .call::<Getsockopt>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([unix_socket as u64, SOL_SOCKET, SO_PASSCRED, 0, 4, 0])
            .call::<Setsockopt>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([unix_socket as u64, SOL_SOCKET, SO_ERROR, page + 896, 4, 0])
            .call::<Setsockopt>(),
        SyscallError::InvalidArguments,
    );

    let inet_socket =
        expect_fd(SyscallArgs::new([AF_INET, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
    write_user_value(page + 96, &111u32);
    expect_ok(
        SyscallArgs::new([inet_socket as u64, page + 640, page + 96, 0, 0, 0])
            .call::<Getsockname>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(page + 96), 16);
    let inet_name = read_user_value::<TestLinuxSockAddrIn>(page + 640);
    assert_eq!(inet_name.sin_family, AF_INET as u16);
    assert_eq!(inet_name.sin_port, 0);
    assert_eq!(inet_name.sin_addr, [0, 0, 0, 0]);
    assert_eq!(inet_name.sin_zero, [0; 8]);

    expect_errno(
        SyscallArgs::new([inet_socket as u64, page + 768, page + 96, 0, 0, 0])
            .call::<Getpeername>(),
        SyscallError::NotConnected,
    );
    write_user_value(page + 968, &4u32);
    expect_ok(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_TYPE,
            page + 976,
            page + 968,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 976), SOCK_DGRAM as i32);
    write_user_value(page + 984, &4u32);
    expect_ok(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_PROTOCOL,
            page + 992,
            page + 984,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 992), 17);
    write_user_value(page + 1000, &4u32);
    expect_ok(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_ACCEPTCONN,
            page + 1008,
            page + 1000,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1008), 0);
    write_user_value(page + 1016, &4u32);
    expect_ok(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_DOMAIN,
            page + 1024,
            page + 1016,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1024), AF_INET as i32);
    write_user_value(page + 1032, &4u32);
    expect_ok(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_ERROR,
            page + 1040,
            page + 1032,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1040), 0);
    write_user_value(page + 1048, &8192i32);
    expect_ok(
        SyscallArgs::new([inet_socket as u64, SOL_SOCKET, SO_SNDBUF, page + 1048, 4, 0])
            .call::<Setsockopt>(),
        0,
    );
    write_user_value(page + 1056, &4i32);
    expect_ok(
        SyscallArgs::new([inet_socket as u64, SOL_TCP, TCP_NODELAY, page + 1056, 4, 0])
            .call::<Setsockopt>(),
        0,
    );
    write_user_value(page + 1064, &4u32);
    expect_ok(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_TCP,
            TCP_NODELAY,
            page + 1072,
            page + 1064,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1072), 1);
    expect_errno(
        SyscallArgs::new([inet_socket as u64, SOL_TCP, 99, page + 1056, 4, 0]).call::<Setsockopt>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([inet_socket as u64, SOL_TCP, 99, page + 1072, page + 1064, 0])
            .call::<Getsockopt>(),
        SyscallError::InvalidArguments,
    );

    let netlink_socket = expect_fd(
        SyscallArgs::new([
            AF_NETLINK,
            SOCK_RAW | SOCK_NONBLOCK | SOCK_CLOEXEC,
            0,
            0,
            0,
            0,
        ])
        .call::<Socket>(),
    );
    assert_fd_flags(netlink_socket, FdFlags::CLOEXEC);
    assert_object_flags(netlink_socket, FileFlags::NONBLOCK);
    write_user_value(page + 1080, &4u32);
    expect_ok(
        SyscallArgs::new([
            netlink_socket as u64,
            SOL_SOCKET,
            SO_TYPE,
            page + 1088,
            page + 1080,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1088), SOCK_RAW as i32);
    write_user_value(page + 1096, &4u32);
    expect_ok(
        SyscallArgs::new([
            netlink_socket as u64,
            SOL_SOCKET,
            SO_DOMAIN,
            page + 1104,
            page + 1096,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1104), AF_NETLINK as i32);
    write_user_value(page + 1112, &4u32);
    expect_ok(
        SyscallArgs::new([
            netlink_socket as u64,
            SOL_SOCKET,
            SO_PROTOCOL,
            page + 1120,
            page + 1112,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1120), 0);
    write_user_value(page + 1128, &1i32);
    expect_ok(
        SyscallArgs::new([
            netlink_socket as u64,
            SOL_SOCKET,
            SO_PASSCRED,
            page + 1128,
            4,
            0,
        ])
        .call::<Setsockopt>(),
        0,
    );
    write_user_value(page + 1136, &4u32);
    expect_ok(
        SyscallArgs::new([
            netlink_socket as u64,
            SOL_SOCKET,
            SO_PASSCRED,
            page + 1144,
            page + 1136,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1144), 1);

    expect_errno(
        SyscallArgs::new([99, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>(),
        SyscallError::AddressFamilyNotSupported,
    );
    expect_errno(
        SyscallArgs::new([AF_INET, SOCK_STREAM, 17, 0, 0, 0]).call::<Socket>(),
        SyscallError::ProtocolNotSupported,
    );
    expect_errno(
        SyscallArgs::new([AF_NETLINK, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>(),
        SyscallError::ProtocolNotSupported,
    );

    close_test_fd(netlink_socket);
    close_test_fd(inet_socket);
    close_test_fd(unix_socket);
    close_test_fd(right_fd);
    close_test_fd(left_fd);
}

fn socket_bind_connect_accept_syscalls_follow_linux_rules() {
    const AF_INET: u64 = 2;
    const AF_UNIX: u64 = 1;
    const SOCK_STREAM: u64 = 1;
    const SOCK_DGRAM: u64 = 2;
    const SOCK_NONBLOCK: u64 = 0o0004000;
    const SOCK_CLOEXEC: u64 = 0o2000000;

    assert_linux_layout::<TestLinuxSockAddrUn>(110, 2);
    assert_linux_layout::<TestLinuxSockAddrIn>(16, 2);

    let page = allocate_user_test_page();
    let socket_path = b"/tmp/accept4-linux.sock\0";
    let missing_socket_path = b"/tmp/accept4-missing.sock\0";
    write_user_value(page, socket_path);
    write_user_value(page + 384, missing_socket_path);

    let mut unix_addr = TestLinuxSockAddrUn::default();
    unix_addr.sun_family = AF_UNIX as u16;
    unix_addr.sun_path[..socket_path.len()].copy_from_slice(socket_path);
    write_user_value(page + 128, &unix_addr);
    let mut missing_unix_addr = TestLinuxSockAddrUn::default();
    missing_unix_addr.sun_family = AF_UNIX as u16;
    missing_unix_addr.sun_path[..missing_socket_path.len()].copy_from_slice(missing_socket_path);
    write_user_value(page + 640, &missing_unix_addr);

    let server = expect_fd(
        SyscallArgs::new([AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK, 0, 0, 0, 0]).call::<Socket>(),
    );
    expect_ok(
        SyscallArgs::new([server as u64, page + 128, 110, 0, 0, 0]).call::<Bind>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([server as u64, page + 128, 110, 0, 0, 0]).call::<Bind>(),
        SyscallError::InvalidArguments,
    );
    let occupied = expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
    expect_errno(
        SyscallArgs::new([occupied as u64, page + 128, 110, 0, 0, 0]).call::<Bind>(),
        SyscallError::AddressInUse,
    );
    expect_ok(
        SyscallArgs::new([server as u64, 0, 0, 0, 0, 0]).call::<Listen>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([server as u64, page + 256, page + 264, SOCK_NONBLOCK, 0, 0])
            .call::<Accept4>(),
        SyscallError::TryAgain,
    );

    let client = expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
    expect_ok(
        SyscallArgs::new([client as u64, page + 128, 110, 0, 0, 0]).call::<Connect>(),
        0,
    );

    write_user_value(page + 264, &2u32);
    let accepted = expect_fd(
        SyscallArgs::new([
            server as u64,
            page + 256,
            page + 264,
            SOCK_NONBLOCK | SOCK_CLOEXEC,
            0,
            0,
        ])
        .call::<Accept4>(),
    );
    assert_fd_flags(accepted, FdFlags::CLOEXEC);
    assert_object_flags(accepted, FileFlags::NONBLOCK);
    assert_eq!(read_user_value::<u32>(page + 264), 2);
    let peer = read_user_value::<TestLinuxSockAddrUn>(page + 256);
    assert_eq!(peer.sun_family, AF_UNIX as u16);

    let rebound = expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
    expect_errno(
        SyscallArgs::new([rebound as u64, page + 128, 1, 0, 0, 0]).call::<Bind>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([rebound as u64, 0, 110, 0, 0, 0]).call::<Bind>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([rebound as u64, 0, 0, 0, 0, 0]).call::<Connect>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([rebound as u64, page + 640, 110, 0, 0, 0]).call::<Connect>(),
        SyscallError::ConnectionRefused,
    );

    let unix_dgram =
        expect_fd(SyscallArgs::new([AF_UNIX, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
    expect_errno(
        SyscallArgs::new([unix_dgram as u64, 1, 0, 0, 0, 0]).call::<Listen>(),
        SyscallError::InvalidArguments,
    );

    let inet_stream = expect_fd(
        SyscallArgs::new([AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0, 0, 0, 0]).call::<Socket>(),
    );
    let inet_any = TestLinuxSockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: 0,
        sin_addr: [0, 0, 0, 0],
        sin_zero: [0; 8],
    };
    write_user_value(page + 512, &inet_any);
    expect_errno(
        SyscallArgs::new([inet_stream as u64, page + 512, 16, 0, 0, 0]).call::<Bind>(),
        SyscallError::AddressNotAvailable,
    );
    expect_errno(
        SyscallArgs::new([inet_stream as u64, 1, 0, 0, 0, 0]).call::<Listen>(),
        SyscallError::AddressNotAvailable,
    );
    expect_errno(
        SyscallArgs::new([inet_stream as u64, page + 512, 16, 0, 0, 0]).call::<Connect>(),
        SyscallError::ConnectionRefused,
    );

    let inet_dgram =
        expect_fd(SyscallArgs::new([AF_INET, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
    expect_errno(
        SyscallArgs::new([inet_dgram as u64, 1, 0, 0, 0, 0]).call::<Listen>(),
        SyscallError::OperationNotSupported,
    );
    expect_errno(
        SyscallArgs::new([inet_dgram as u64, page + 512, 16, 0, 0, 0]).call::<Connect>(),
        SyscallError::ConnectionRefused,
    );
    expect_errno(
        SyscallArgs::new([inet_dgram as u64, page + 256, page + 264, 0, 0, 0]).call::<Accept4>(),
        SyscallError::OperationNotSupported,
    );

    close_test_fd(inet_dgram);
    close_test_fd(inet_stream);
    close_test_fd(unix_dgram);
    close_test_fd(rebound);
    close_test_fd(occupied);
    close_test_fd(accepted);
    close_test_fd(client);
    close_test_fd(server);
}

fn socket_message_syscalls_follow_linux_rules() {
    const AF_UNIX: u64 = 1;
    const SOCK_STREAM: u64 = 1;
    const SOCK_DGRAM: u64 = 2;
    const SOCK_NONBLOCK: u64 = 0o0004000;
    const SOL_SOCKET: i32 = 1;
    const SO_PASSCRED: u64 = 16;
    const SCM_RIGHTS: i32 = 1;
    const SCM_CREDENTIALS: i32 = 2;
    const MSG_CTRUNC: i32 = 0x8;
    const MSG_CMSG_CLOEXEC: u64 = 0x4000_0000;

    assert_linux_layout::<TestRelibcIovec>(16, 8);
    assert_linux_layout::<TestRelibcMsgHdr>(56, 8);
    assert_linux_layout::<TestRelibcMmsghdr>(64, 8);
    assert_linux_layout::<TestLinuxCmsgHdr>(16, 8);
    assert_linux_layout::<TestRightsControlMessage>(24, 8);

    let page = allocate_user_test_page();
    let listener_path = b"/tmp/accept-linux.sock\0";
    let source_path = b"/tmp/sendto-src.sock\0";
    let target_path = b"/tmp/sendto-dst.sock\0";
    write_user_value(page, listener_path);
    write_user_value(page + 256, source_path);
    write_user_value(page + 512, target_path);

    let mut listener_addr = TestLinuxSockAddrUn::default();
    listener_addr.sun_family = AF_UNIX as u16;
    listener_addr.sun_path[..listener_path.len()].copy_from_slice(listener_path);
    write_user_value(page + 128, &listener_addr);

    let mut source_addr = TestLinuxSockAddrUn::default();
    source_addr.sun_family = AF_UNIX as u16;
    source_addr.sun_path[..source_path.len()].copy_from_slice(source_path);
    write_user_value(page + 384, &source_addr);

    let mut target_addr = TestLinuxSockAddrUn::default();
    target_addr.sun_family = AF_UNIX as u16;
    target_addr.sun_path[..target_path.len()].copy_from_slice(target_path);
    write_user_value(page + 640, &target_addr);

    let listener = expect_fd(
        SyscallArgs::new([AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK, 0, 0, 0, 0]).call::<Socket>(),
    );
    expect_ok(
        SyscallArgs::new([listener as u64, page + 128, 110, 0, 0, 0]).call::<Bind>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([listener as u64, 4, 0, 0, 0, 0]).call::<Listen>(),
        0,
    );
    let client = expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
    expect_ok(
        SyscallArgs::new([client as u64, page + 128, 110, 0, 0, 0]).call::<Connect>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([listener as u64, page + 768, 0, 0, 0, 0]).call::<Accept>(),
        SyscallError::BadAddress,
    );
    write_user_value(page + 776, &2u32);
    let accepted = expect_fd(
        SyscallArgs::new([listener as u64, page + 768, page + 776, 0, 0, 0]).call::<Accept>(),
    );
    assert_fd_flags(accepted, FdFlags::empty());
    assert_object_flags(accepted, FileFlags::empty());
    assert_eq!(read_user_value::<u32>(page + 776), 2);
    let accepted_peer = read_user_value::<TestLinuxSockAddrUn>(page + 768);
    assert_eq!(accepted_peer.sun_family, AF_UNIX as u16);

    let sender = expect_fd(SyscallArgs::new([AF_UNIX, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
    let receiver = expect_fd(SyscallArgs::new([AF_UNIX, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
    expect_ok(
        SyscallArgs::new([sender as u64, page + 384, 110, 0, 0, 0]).call::<Bind>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([receiver as u64, page + 640, 110, 0, 0, 0]).call::<Bind>(),
        0,
    );

    write_user_value(page + 896, b"hey");
    expect_ok(
        SyscallArgs::new([sender as u64, page + 896, 3, 0, page + 640, 110]).call::<Sendto>(),
        3,
    );
    write_user_value(page + 1048, &2u32);
    expect_ok(
        SyscallArgs::new([receiver as u64, page + 1024, 8, 0, page + 1152, page + 1048])
            .call::<Recvfrom>(),
        3,
    );
    assert_user_bytes(page + 1024, b"hey");
    assert_eq!(read_user_value::<u32>(page + 1048), 110);
    let recv_source = read_user_value::<TestLinuxSockAddrUn>(page + 1152);
    assert_eq!(recv_source.sun_family, AF_UNIX as u16);
    expect_errno(
        SyscallArgs::new([sender as u64, page + 896, 1, 0, page + 640, 1]).call::<Sendto>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([sender as u64, 0, 1, 0, page + 640, 110]).call::<Sendto>(),
        SyscallError::BadAddress,
    );

    let rights_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let socketpair_page = page + 1408;
    expect_ok(
        SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, socketpair_page, 0, 0]).call::<Socketpair>(),
        0,
    );
    let stream_left = read_user_value::<i32>(socketpair_page) as usize;
    let stream_right = read_user_value::<i32>(socketpair_page + 4) as usize;

    write_user_value(page + 1424, &[b'R']);
    let send_iov = TestRelibcIovec {
        iov_base: (page + 1424) as *mut u8,
        iov_len: 1,
    };
    write_user_value(page + 1440, &send_iov);
    let send_control = TestRightsControlMessage {
        header: TestLinuxCmsgHdr {
            cmsg_len: 20,
            cmsg_level: SOL_SOCKET,
            cmsg_type: SCM_RIGHTS,
        },
        fd: rights_fd as i32,
        pad: 0,
    };
    write_user_value(page + 1472, &send_control);
    let send_msg = TestRelibcMsgHdr {
        msg_name: core::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: (page + 1440) as *mut TestRelibcIovec,
        msg_iovlen: 1,
        msg_control: (page + 1472) as *mut u8,
        msg_controllen: core::mem::size_of::<TestRightsControlMessage>(),
        msg_flags: 0,
    };
    write_user_value(page + 1504, &send_msg);
    expect_ok(
        SyscallArgs::new([stream_left as u64, page + 1504, 0, 0, 0, 0]).call::<Sendmsg>(),
        1,
    );

    write_user_value(page + 1568, &[0u8]);
    let recv_iov = TestRelibcIovec {
        iov_base: (page + 1568) as *mut u8,
        iov_len: 1,
    };
    write_user_value(page + 1584, &recv_iov);
    write_user_value(page + 1616, &TestRightsControlMessage::default());
    let recv_msg = TestRelibcMsgHdr {
        msg_name: core::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: (page + 1584) as *mut TestRelibcIovec,
        msg_iovlen: 1,
        msg_control: (page + 1616) as *mut u8,
        msg_controllen: core::mem::size_of::<TestRightsControlMessage>(),
        msg_flags: 0,
    };
    write_user_value(page + 1648, &recv_msg);
    expect_ok(
        SyscallArgs::new([stream_right as u64, page + 1648, MSG_CMSG_CLOEXEC, 0, 0, 0])
            .call::<Recvmsg>(),
        1,
    );
    assert_user_bytes(page + 1568, b"R");
    let recv_msg_after = read_user_value::<TestRelibcMsgHdr>(page + 1648);
    assert_eq!(recv_msg_after.msg_flags, 0);
    assert_eq!(
        recv_msg_after.msg_controllen,
        core::mem::size_of::<TestRightsControlMessage>()
    );
    let received_control = read_user_value::<TestRightsControlMessage>(page + 1616);
    assert_eq!(received_control.header.cmsg_len, 20);
    assert_eq!(received_control.header.cmsg_level, SOL_SOCKET);
    assert_eq!(received_control.header.cmsg_type, SCM_RIGHTS);
    let received_fd =
        usize::try_from(received_control.fd).expect("received fd should be non-negative");
    assert_ne!(received_fd, rights_fd);
    assert_fd_flags(received_fd, FdFlags::CLOEXEC);
    assert_same_object(received_fd, rights_fd);

    write_user_value(page + 1680, &1i32);
    expect_ok(
        SyscallArgs::new([
            stream_right as u64,
            SOL_SOCKET as u64,
            SO_PASSCRED,
            page + 1680,
            4,
            0,
        ])
        .call::<Setsockopt>(),
        0,
    );
    write_user_value(page + 2048, b"C");
    expect_ok(
        SyscallArgs::new([stream_left as u64, page + 2048, 1, 0, 0, 0]).call::<Write>(),
        1,
    );
    write_user_value(page + 2064, &[0u8]);
    let cred_recv_iov = TestRelibcIovec {
        iov_base: (page + 2064) as *mut u8,
        iov_len: 1,
    };
    write_user_value(page + 2080, &cred_recv_iov);
    write_user_value(page + 2112, &[0u8; 32]);
    let cred_recv_msg = TestRelibcMsgHdr {
        msg_name: core::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: (page + 2080) as *mut TestRelibcIovec,
        msg_iovlen: 1,
        msg_control: (page + 2112) as *mut u8,
        msg_controllen: 32,
        msg_flags: 0,
    };
    write_user_value(page + 2160, &cred_recv_msg);
    expect_ok(
        SyscallArgs::new([stream_right as u64, page + 2160, 0, 0, 0, 0]).call::<Recvmsg>(),
        1,
    );
    assert_user_bytes(page + 2064, b"C");
    let cred_recv_after = read_user_value::<TestRelibcMsgHdr>(page + 2160);
    assert_eq!(cred_recv_after.msg_flags, 0);
    assert_eq!(cred_recv_after.msg_controllen, 32);
    let credential_control = read_user_value::<TestLinuxCmsgHdr>(page + 2112);
    assert_eq!(credential_control.cmsg_len, 28);
    assert_eq!(credential_control.cmsg_level, SOL_SOCKET);
    assert_eq!(credential_control.cmsg_type, SCM_CREDENTIALS);
    let received_cred = read_user_value::<TestLinuxUcred>(page + 2128);
    let current = get_current_process();
    let current = current.lock();
    assert_eq!(received_cred.pid, current.pid.0 as i32);
    assert_eq!(received_cred.uid, current.effective_uid);
    assert_eq!(received_cred.gid, current.effective_gid);
    drop(current);

    write_user_value(page + 2208, b"T");
    expect_ok(
        SyscallArgs::new([stream_left as u64, page + 2208, 1, 0, 0, 0]).call::<Write>(),
        1,
    );
    write_user_value(page + 2224, &[0u8]);
    let trunc_recv_iov = TestRelibcIovec {
        iov_base: (page + 2224) as *mut u8,
        iov_len: 1,
    };
    write_user_value(page + 2240, &trunc_recv_iov);
    write_user_value(page + 2272, &TestLinuxCmsgHdr::default());
    let trunc_recv_msg = TestRelibcMsgHdr {
        msg_name: core::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: (page + 2240) as *mut TestRelibcIovec,
        msg_iovlen: 1,
        msg_control: (page + 2272) as *mut u8,
        msg_controllen: core::mem::size_of::<TestLinuxCmsgHdr>(),
        msg_flags: 0,
    };
    write_user_value(page + 2304, &trunc_recv_msg);
    expect_ok(
        SyscallArgs::new([stream_right as u64, page + 2304, 0, 0, 0, 0]).call::<Recvmsg>(),
        1,
    );
    assert_user_bytes(page + 2224, b"T");
    let trunc_recv_after = read_user_value::<TestRelibcMsgHdr>(page + 2304);
    assert_eq!(trunc_recv_after.msg_flags & MSG_CTRUNC, MSG_CTRUNC);
    assert_eq!(
        trunc_recv_after.msg_controllen,
        core::mem::size_of::<TestLinuxCmsgHdr>()
    );

    expect_errno(
        SyscallArgs::new([stream_left as u64, 0, 0, 0, 0, 0]).call::<Sendmsg>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([stream_right as u64, 0, 0, 0, 0, 0]).call::<Recvmsg>(),
        SyscallError::BadAddress,
    );

    let dgram_pair_page = page + 1728;
    expect_ok(
        SyscallArgs::new([AF_UNIX, SOCK_DGRAM, 0, dgram_pair_page, 0, 0]).call::<Socketpair>(),
        0,
    );
    let dgram_left = read_user_value::<i32>(dgram_pair_page) as usize;
    let dgram_right = read_user_value::<i32>(dgram_pair_page + 4) as usize;
    write_user_value(page + 1744, b"go");
    write_user_value(page + 1760, b"again");
    let sendmmsg_iov = [
        TestRelibcIovec {
            iov_base: (page + 1744) as *mut u8,
            iov_len: 2,
        },
        TestRelibcIovec {
            iov_base: (page + 1760) as *mut u8,
            iov_len: 5,
        },
    ];
    write_user_value(page + 1792, &sendmmsg_iov);
    let msgvec = [
        TestRelibcMmsghdr {
            msg_hdr: TestRelibcMsgHdr {
                msg_name: core::ptr::null_mut(),
                msg_namelen: 0,
                msg_iov: (page + 1792) as *mut TestRelibcIovec,
                msg_iovlen: 1,
                msg_control: core::ptr::null_mut(),
                msg_controllen: 0,
                msg_flags: 0,
            },
            msg_len: 0,
        },
        TestRelibcMmsghdr {
            msg_hdr: TestRelibcMsgHdr {
                msg_name: core::ptr::null_mut(),
                msg_namelen: 0,
                msg_iov: (page + 1792 + core::mem::size_of::<TestRelibcIovec>() as u64)
                    as *mut TestRelibcIovec,
                msg_iovlen: 1,
                msg_control: core::ptr::null_mut(),
                msg_controllen: 0,
                msg_flags: 0,
            },
            msg_len: 0,
        },
    ];
    write_user_value(page + 1856, &msgvec);
    expect_ok(
        SyscallArgs::new([dgram_left as u64, page + 1856, 2, 0, 0, 0]).call::<Sendmmsg>(),
        2,
    );
    let sent_vec = read_user_value::<[TestRelibcMmsghdr; 2]>(page + 1856);
    assert_eq!(sent_vec[0].msg_len, 2);
    assert_eq!(sent_vec[1].msg_len, 5);
    expect_ok(
        SyscallArgs::new([dgram_right as u64, page + 2000, 8, 0, 0, 0]).call::<Recvfrom>(),
        2,
    );
    assert_user_bytes(page + 2000, b"go");
    expect_ok(
        SyscallArgs::new([dgram_right as u64, page + 2016, 8, 0, 0, 0]).call::<Recvfrom>(),
        5,
    );
    assert_user_bytes(page + 2016, b"again");
    expect_errno(
        SyscallArgs::new([dgram_left as u64, 0, 1, 0, 0, 0]).call::<Sendmmsg>(),
        SyscallError::BadAddress,
    );

    close_test_fd(dgram_right);
    close_test_fd(dgram_left);
    close_test_fd(received_fd);
    close_test_fd(stream_right);
    close_test_fd(stream_left);
    close_test_fd(rights_fd);
    close_test_fd(receiver);
    close_test_fd(sender);
    close_test_fd(accepted);
    close_test_fd(client);
    close_test_fd(listener);
}

fn namespace_and_kcmp_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const CLONE_NEWNET: u64 = 0x4000_0000;

    let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let saved_namespace = get_current_process().lock().net_namespace.clone();
    let proc_ns = allocate_user_test_page();
    write_user_cstr(proc_ns, b"/proc/self/ns/net\0");
    let ns_fd = expect_fd(
        SyscallArgs::new([AT_FDCWD, proc_ns, OpenFlags::empty().bits() as u64, 0, 0, 0])
            .call::<OpenAt>(),
    );
    let original_inode = saved_namespace.inode();
    expect_ok(
        SyscallArgs::new([CLONE_NEWNET, 0, 0, 0, 0, 0]).call::<Unshare>(),
        0,
    );
    let new_inode = get_current_process().lock().net_namespace.inode();
    assert_ne!(new_inode, original_inode);
    expect_ok(
        SyscallArgs::new([ns_fd as u64, 0, 0, 0, 0, 0]).call::<Setns>(),
        0,
    );
    assert_eq!(
        get_current_process().lock().net_namespace.inode(),
        original_inode
    );
    expect_ok(
        SyscallArgs::new([ns_fd as u64, CLONE_NEWNET, 0, 0, 0, 0]).call::<Setns>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([eventfd as u64, 0, 0, 0, 0, 0]).call::<Setns>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([ns_fd as u64, 0x2000_0000, 0, 0, 0, 0]).call::<Setns>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0x8000_0000, 0, 0, 0, 0, 0]).call::<Unshare>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0x20000, 0, 0, 0, 0, 0]).call::<Unshare>(),
        SyscallError::OperationNotSupported,
    );

    let saved_process = get_current_process();
    let other_eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let kcmp_process = Process::init();
    let kcmp_pid = kcmp_process.lock().pid.0 as u64;
    MANAGER.lock().processes.insert(
        crate::process::misc::ProcessID(kcmp_pid),
        kcmp_process.clone(),
    );
    let same_object;
    let other_kcmp_fd;
    {
        let mut process = kcmp_process.lock();
        same_object = (
            process.push_object_with_flags(
                get_object_current_process(eventfd as u64).unwrap(),
                FdFlags::empty(),
            ),
            process.push_object_with_flags(
                get_object_current_process(eventfd as u64).unwrap(),
                FdFlags::empty(),
            ),
        );
        other_kcmp_fd = process.push_object_with_flags(
            get_object_current_process(other_eventfd as u64).unwrap(),
            FdFlags::empty(),
        );
    }
    set_current_process(Some(kcmp_process.clone()));
    let kcmp_equal = SyscallArgs::new([
        kcmp_pid,
        kcmp_pid,
        0,
        same_object.0 as u64,
        same_object.1 as u64,
        0,
    ])
    .call::<Kcmp>();
    expect_ok(kcmp_equal, 0);
    let cmp_ab = SyscallArgs::new([
        kcmp_pid,
        kcmp_pid,
        0,
        same_object.0 as u64,
        other_kcmp_fd as u64,
        0,
    ])
    .call::<Kcmp>()
    .expect("kcmp should compare file objects");
    let cmp_ba = SyscallArgs::new([
        kcmp_pid,
        kcmp_pid,
        0,
        other_kcmp_fd as u64,
        same_object.0 as u64,
        0,
    ])
    .call::<Kcmp>()
    .expect("kcmp reverse should compare file objects");
    assert!(matches!((cmp_ab, cmp_ba), (1, 2) | (2, 1)));
    expect_errno(
        SyscallArgs::new([
            0,
            kcmp_pid,
            0,
            same_object.0 as u64,
            other_kcmp_fd as u64,
            0,
        ])
        .call::<Kcmp>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([
            kcmp_pid + 1,
            kcmp_pid,
            0,
            same_object.0 as u64,
            other_kcmp_fd as u64,
            0,
        ])
        .call::<Kcmp>(),
        SyscallError::PermissionDenied,
    );
    expect_errno(
        SyscallArgs::new([
            kcmp_pid,
            kcmp_pid,
            99,
            same_object.0 as u64,
            other_kcmp_fd as u64,
            0,
        ])
        .call::<Kcmp>(),
        SyscallError::InvalidArguments,
    );
    set_current_process(Some(saved_process));
    MANAGER
        .lock()
        .processes
        .remove(&crate::process::misc::ProcessID(kcmp_pid));

    close_test_fd(other_eventfd);
    close_test_fd(ns_fd);
    close_test_fd(eventfd);
    get_current_process().lock().net_namespace = saved_namespace;
}

fn close_range_syscalls_follow_linux_rules() {
    const CLOSE_RANGE_CLOEXEC: u64 = 0x4;

    let base_count = occupied_fd_count();
    let fd0 = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let fd1 = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let fd2 = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let fd3 = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());

    expect_ok(
        SyscallArgs::new([fd1 as u64, fd2 as u64, CLOSE_RANGE_CLOEXEC, 0, 0, 0])
            .call::<CloseRange>(),
        0,
    );
    assert_fd_flags(fd0, FdFlags::empty());
    assert_fd_flags(fd1, FdFlags::CLOEXEC);
    assert_fd_flags(fd2, FdFlags::CLOEXEC);
    assert_fd_flags(fd3, FdFlags::empty());
    assert_eq!(occupied_fd_count(), base_count + 4);

    expect_ok(
        SyscallArgs::new([fd1 as u64, fd2 as u64, 0, 0, 0, 0]).call::<CloseRange>(),
        0,
    );
    assert!(get_object_current_process(fd1 as u64).is_err());
    assert!(get_object_current_process(fd2 as u64).is_err());
    assert_eq!(occupied_fd_count(), base_count + 2);

    expect_errno(
        SyscallArgs::new([fd0 as u64, fd3 as u64, 1, 0, 0, 0]).call::<CloseRange>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([fd3 as u64, fd0 as u64, 0, 0, 0, 0]).call::<CloseRange>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(
        SyscallArgs::new([4096, 8192, 0, 0, 0, 0]).call::<CloseRange>(),
        0,
    );

    close_test_fd(fd0);
    close_test_fd(fd3);
}

fn pidfd_and_waitid_syscalls_follow_linux_rules() {
    const P_PID: u64 = 1;
    const P_PIDFD: u64 = 3;
    const EPOLL_CTL_ADD: u64 = 1;
    const EPOLLIN: u32 = 0x001;
    const POLLIN: i16 = 0x001;
    const POLLHUP: i16 = 0x010;
    const WNOHANG: u64 = 1;
    const WUNTRACED: u64 = 2;
    const WSTOPPED: u64 = 2;
    const WEXITED: u64 = 4;
    const WBAD: u64 = 0x20;
    const __WCLONE: u64 = 0x8000_0000;
    const WNOWAIT: u64 = 0x0100_0000;
    const CLD_EXITED: i32 = 1;
    const SI_QUEUE: i32 = -1;
    const STOP_STATUS: i32 = 0x7f;

    assert_linux_layout::<TestWaitidSigInfo>(128, 8);
    assert_linux_layout::<TestLinuxRusage>(144, 8);

    let current = get_current_process();

    let child = Process::empty();
    let child_pid = {
        let mut child = child.lock();
        child.pid = ProcessID::new();
        child.parent = Some(current.clone());
        child.group_id = current.lock().group_id;
        child.pid.0
    };
    MANAGER
        .lock()
        .processes
        .insert(ProcessID(child_pid), child.clone());

    let child_pidfd = expect_fd(SyscallArgs::new([child_pid, 0, 0, 0, 0, 0]).call::<PidfdOpen>());
    assert_fd_flags(child_pidfd, FdFlags::CLOEXEC);
    assert!(
        get_object_current_process(child_pidfd as u64)
            .expect("pidfd should resolve")
            .as_pidfd()
            .is_ok()
    );
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<PidfdOpen>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([child_pid, 1, 0, 0, 0, 0]).call::<PidfdOpen>(),
        SyscallError::InvalidArguments,
    );
    let info_page = allocate_user_test_page();
    let poll_page = info_page + 512;
    write_user_value(
        poll_page,
        &[TestLinuxPollFd {
            fd: child_pidfd as i32,
            events: POLLIN | POLLHUP,
            revents: -1,
        }],
    );
    expect_ok(
        SyscallArgs::new([poll_page, 1, 0, 0, 0, 0]).call::<Poll>(),
        0,
    );
    assert_eq!(read_user_value::<TestLinuxPollFd>(poll_page).revents, 0);

    child.lock().exit_status = Some(ProcessExitStatus::Exited(7));
    write_user_value(
        poll_page,
        &[TestLinuxPollFd {
            fd: child_pidfd as i32,
            events: POLLIN | POLLHUP,
            revents: 0,
        }],
    );
    expect_ok(
        SyscallArgs::new([poll_page, 1, 0, 0, 0, 0]).call::<Poll>(),
        1,
    );
    let pidfd_poll = read_user_value::<TestLinuxPollFd>(poll_page);
    assert_eq!(pidfd_poll.revents & POLLIN, POLLIN);
    assert_eq!(pidfd_poll.revents & POLLHUP, 0);

    let pidfd_epoll = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<EpollCreate1>());
    let pidfd_event = TestLinuxEpollEvent {
        events: EPOLLIN,
        data: 0x7069_6466,
    };
    write_user_value(info_page + 640, &pidfd_event);
    expect_ok(
        SyscallArgs::new([
            pidfd_epoll as u64,
            EPOLL_CTL_ADD,
            child_pidfd as u64,
            info_page + 640,
            0,
            0,
        ])
        .call::<EpollCtl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([pidfd_epoll as u64, info_page + 704, 1, 0, 0, 0]).call::<EpollWait>(),
        1,
    );
    let pidfd_ready = read_user_value::<TestLinuxEpollEvent>(info_page + 704);
    let pidfd_ready_events = pidfd_ready.events;
    let pidfd_ready_data = pidfd_ready.data;
    assert_eq!(pidfd_ready_events & EPOLLIN, EPOLLIN);
    assert_eq!(pidfd_ready_data, 0x7069_6466);
    close_test_fd(pidfd_epoll);

    expect_ok(
        SyscallArgs::new([
            P_PIDFD,
            child_pidfd as u64,
            info_page,
            WEXITED | WNOWAIT,
            0,
            0,
        ])
        .call::<Waitid>(),
        0,
    );
    let info = read_user_value::<TestWaitidSigInfo>(info_page);
    assert_eq!(info.si_signo, Signal::SIGCHLD as i32);
    assert_eq!(info.si_code, CLD_EXITED);
    assert_eq!(info.si_pid, child_pid as i32);
    assert_eq!(info.si_status, 7);
    assert!(MANAGER.lock().processes.contains_key(&ProcessID(child_pid)));

    let current_pid = get_current_process().lock().pid.0 as i32;
    let current_uid = get_current_process().lock().real_uid;
    let mut queued_siginfo = SigInfo::for_process_signal(Signal::SIGUSR1, current_pid, current_uid);
    queued_siginfo.si_code = SI_QUEUE;
    write_user_value(info_page + 128, &queued_siginfo);
    expect_ok(
        SyscallArgs::new([
            child_pidfd as u64,
            Signal::SIGUSR1 as u64,
            info_page + 128,
            0,
            0,
            0,
        ])
        .call::<PidfdSendSignal>(),
        0,
    );
    {
        let child = child.lock();
        assert!(
            child
                .pending_signals
                .contains(Signals::from(Signal::SIGUSR1))
        );
        let pending = child.pending_signal_info[Signal::SIGUSR1.index()]
            .expect("siginfo should be stored for pidfd_send_signal");
        assert_eq!(pending.si_signo, Signal::SIGUSR1 as i32);
        assert_eq!(pending.si_code, SI_QUEUE);
        assert_eq!(pending.si_pid, current_pid);
        assert_eq!(pending.si_uid, current_uid);
    }
    expect_ok(
        SyscallArgs::new([child_pidfd as u64, 0, 0, 0, 0, 0]).call::<PidfdSendSignal>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([child_pidfd as u64, 0, info_page + 128, 0, 0, 0])
            .call::<PidfdSendSignal>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([
            child_pidfd as u64,
            Signal::SIGUSR1 as u64,
            info_page + 128,
            1,
            0,
            0,
        ])
        .call::<PidfdSendSignal>(),
        SyscallError::InvalidArguments,
    );

    expect_ok(
        SyscallArgs::new([P_PID, child_pid, info_page, WEXITED | WNOHANG, 0, 0]).call::<Waitid>(),
        0,
    );
    assert!(!MANAGER.lock().processes.contains_key(&ProcessID(child_pid)));

    write_user_value(info_page + 256, &0x55aa55aai32);
    write_user_value(info_page + 320, &[0xa5u8; 144]);

    let wait4_child = Process::empty();
    let wait4_child_pid = {
        let mut child = wait4_child.lock();
        child.pid = ProcessID::new();
        child.parent = Some(current.clone());
        child.group_id = current.lock().group_id;
        child.exit_status = Some(ProcessExitStatus::Exited(9));
        child.pid.0
    };
    MANAGER
        .lock()
        .processes
        .insert(ProcessID(wait4_child_pid), wait4_child.clone());
    expect_ok(
        SyscallArgs::new([
            wait4_child_pid,
            info_page + 256,
            WNOHANG | __WCLONE,
            info_page + 320,
            0,
            0,
        ])
        .call::<Wait4>(),
        wait4_child_pid as usize,
    );
    assert_eq!(read_user_value::<i32>(info_page + 256), 9 << 8);
    assert_eq!(
        read_user_value::<TestLinuxRusage>(info_page + 320).ru_maxrss,
        0
    );
    assert!(
        !MANAGER
            .lock()
            .processes
            .contains_key(&ProcessID(wait4_child_pid))
    );

    let wait4_preserve_child = Process::empty();
    let wait4_preserve_child_pid = {
        let mut child = wait4_preserve_child.lock();
        child.pid = ProcessID::new();
        child.parent = Some(current.clone());
        child.group_id = current.lock().group_id;
        child.exit_status = Some(ProcessExitStatus::Exited(11));
        child.pid.0
    };
    MANAGER.lock().processes.insert(
        ProcessID(wait4_preserve_child_pid),
        wait4_preserve_child.clone(),
    );
    expect_ok(
        SyscallArgs::new([wait4_preserve_child_pid, 0, WNOHANG, 0, 0, 0]).call::<Wait4>(),
        wait4_preserve_child_pid as usize,
    );
    assert!(
        !MANAGER
            .lock()
            .processes
            .contains_key(&ProcessID(wait4_preserve_child_pid))
    );

    let stopped_child = Process::empty();
    let stopped_child_pid = {
        let mut child = stopped_child.lock();
        child.pid = ProcessID::new();
        child.parent = Some(current.clone());
        child.group_id = current.lock().group_id;
        child.wait_event = Some(crate::process::wait::ProcessWaitEvent::Stopped {
            status: STOP_STATUS,
            ptrace: false,
        });
        child.threads.push(alloc::sync::Arc::downgrade(
            &crate::thread::thread::Thread::empty(),
        ));
        child.pid.0
    };
    MANAGER
        .lock()
        .processes
        .insert(ProcessID(stopped_child_pid), stopped_child.clone());

    expect_ok(
        SyscallArgs::new([stopped_child_pid, info_page + 256, WNOHANG, 0, 0, 0]).call::<Wait4>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(info_page + 256), 9 << 8);
    assert!(stopped_child.lock().wait_event.is_some());

    expect_ok(
        SyscallArgs::new([
            stopped_child_pid,
            info_page + 256,
            WNOHANG | WUNTRACED,
            info_page + 320,
            0,
            0,
        ])
        .call::<Wait4>(),
        stopped_child_pid as usize,
    );
    assert_eq!(read_user_value::<i32>(info_page + 256), STOP_STATUS);
    assert_eq!(
        read_user_value::<TestLinuxRusage>(info_page + 320).ru_nivcsw,
        0
    );
    assert!(stopped_child.lock().wait_event.is_none());

    let stopped_child_wnowait = Process::empty();
    let stopped_child_wnowait_pid = {
        let mut child = stopped_child_wnowait.lock();
        child.pid = ProcessID::new();
        child.parent = Some(current.clone());
        child.group_id = current.lock().group_id;
        child.wait_event = Some(crate::process::wait::ProcessWaitEvent::Stopped {
            status: STOP_STATUS,
            ptrace: false,
        });
        child.threads.push(alloc::sync::Arc::downgrade(
            &crate::thread::thread::Thread::empty(),
        ));
        child.pid.0
    };
    MANAGER.lock().processes.insert(
        ProcessID(stopped_child_wnowait_pid),
        stopped_child_wnowait.clone(),
    );
    expect_ok(
        SyscallArgs::new([
            P_PID,
            stopped_child_wnowait_pid,
            info_page,
            WEXITED | WNOWAIT | WSTOPPED,
            0,
            0,
        ])
        .call::<Waitid>(),
        0,
    );
    assert_eq!(read_user_value::<TestWaitidSigInfo>(info_page).si_code, 5);
    assert!(stopped_child_wnowait.lock().wait_event.is_some());
    expect_ok(
        SyscallArgs::new([
            stopped_child_wnowait_pid,
            info_page + 256,
            WNOHANG | WUNTRACED,
            0,
            0,
            0,
        ])
        .call::<Wait4>(),
        stopped_child_wnowait_pid as usize,
    );
    assert_eq!(read_user_value::<i32>(info_page + 256), STOP_STATUS);
    assert!(stopped_child_wnowait.lock().wait_event.is_none());

    let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    expect_errno(
        SyscallArgs::new([99, 0, 0, WEXITED, 0, 0]).call::<Waitid>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([P_PID, current_pid as u64, 0, WNOHANG, 0, 0]).call::<Waitid>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([P_PIDFD, eventfd as u64, 0, WEXITED, 0, 0]).call::<Waitid>(),
        SyscallError::BadFileDescriptor,
    );
    expect_errno(
        SyscallArgs::new([P_PID, child_pid, 0, WEXITED, 0, 0]).call::<Waitid>(),
        SyscallError::NoChildProcesses,
    );
    expect_errno(
        SyscallArgs::new([current_pid as u64, 0, WBAD, 0, 0, 0]).call::<Wait4>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([i32::MIN as u32 as u64, 0, WNOHANG, 0, 0, 0]).call::<Wait4>(),
        SyscallError::NoProcess,
    );
    expect_errno(
        SyscallArgs::new([(current_pid + 10_000) as u64, 0, WNOHANG, 0, 0, 0]).call::<Wait4>(),
        SyscallError::NoChildProcesses,
    );

    MANAGER.lock().processes.remove(&ProcessID(child_pid));
    MANAGER
        .lock()
        .processes
        .remove(&ProcessID(stopped_child_pid));
    MANAGER
        .lock()
        .processes
        .remove(&ProcessID(stopped_child_wnowait_pid));
    close_test_fd(eventfd);
    close_test_fd(child_pidfd);
}

fn sleep_and_signal_mask_syscalls_follow_linux_rules() {
    const SIG_BLOCK: u64 = 0;
    const SIG_UNBLOCK: u64 = 1;
    const SIG_SETMASK: u64 = 2;
    const SS_ONSTACK: i32 = 1;
    const SS_DISABLE: i32 = 2;
    const SA_SIGINFO: u64 = 0x0000_0004;
    const SI_QUEUE: i32 = -1;
    const MINSIGSTKSZ: usize = 2048;

    assert_linux_layout::<TestLinuxStack>(24, 8);
    assert_linux_layout::<TestLinuxSigAction>(32, 8);

    let page = allocate_user_test_page();
    write_user_value(
        page,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
    );
    expect_ok(
        SyscallArgs::new([page, page + 32, 0, 0, 0, 0]).call::<Nanosleep>(),
        0,
    );
    assert_eq!(read_user_value::<TestLinuxTimespec>(page + 32).tv_nsec, 0);
    write_user_value(
        page,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        },
    );
    expect_errno(
        SyscallArgs::new([page, 0, 0, 0, 0, 0]).call::<Nanosleep>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<Nanosleep>(),
        SyscallError::BadAddress,
    );

    write_user_value(page + 64, &TestLinuxItimerval::default());
    expect_ok(
        SyscallArgs::new([0, page + 64, page + 96, 0, 0, 0]).call::<Setitimer>(),
        0,
    );
    assert_eq!(
        read_user_value::<TestLinuxItimerval>(page + 96)
            .it_value
            .tv_sec,
        0
    );
    expect_errno(
        SyscallArgs::new([99, page + 64, 0, 0, 0, 0]).call::<Setitimer>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Setitimer>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Setitimer>(),
        SyscallError::BadAddress,
    );

    let thread = crate::thread::get_current_thread();
    let saved_mask = thread.lock().blocked_signals;
    write_user_value(page + 128, &Signal::SIGUSR1.mask());
    expect_ok(
        SyscallArgs::new([SIG_BLOCK, page + 128, page + 136, 8, 0, 0]).call::<RtSigprocmask>(),
        0,
    );
    assert_eq!(read_user_value::<u64>(page + 136), saved_mask.bits());
    assert!(
        crate::thread::get_current_thread()
            .lock()
            .blocked_signals
            .contains(Signals::from(Signal::SIGUSR1))
    );

    write_user_value(
        page + 144,
        &(Signal::SIGKILL.mask() | Signal::SIGSTOP.mask()),
    );
    expect_ok(
        SyscallArgs::new([SIG_BLOCK, page + 144, 0, 8, 0, 0]).call::<RtSigprocmask>(),
        0,
    );
    let blocked = crate::thread::get_current_thread().lock().blocked_signals;
    assert!(!blocked.contains(Signals::from(Signal::SIGKILL)));
    assert!(!blocked.contains(Signals::from(Signal::SIGSTOP)));

    expect_ok(
        SyscallArgs::new([SIG_UNBLOCK, page + 128, 0, 8, 0, 0]).call::<RtSigprocmask>(),
        0,
    );
    assert!(
        !crate::thread::get_current_thread()
            .lock()
            .blocked_signals
            .contains(Signals::from(Signal::SIGUSR1))
    );

    write_user_value(page + 152, &Signal::SIGTERM.mask());
    expect_ok(
        SyscallArgs::new([SIG_SETMASK, page + 152, 0, 8, 0, 0]).call::<RtSigprocmask>(),
        0,
    );
    assert_eq!(
        crate::thread::get_current_thread()
            .lock()
            .blocked_signals
            .bits(),
        Signal::SIGTERM.mask()
    );
    expect_errno(
        SyscallArgs::new([99, page + 152, 0, 8, 0, 0]).call::<RtSigprocmask>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([SIG_BLOCK, 1, 0, 8, 0, 0]).call::<RtSigprocmask>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([SIG_BLOCK, page + 152, 0, 4, 0, 0]).call::<RtSigprocmask>(),
        SyscallError::InvalidArguments,
    );

    let current = get_current_process();
    let current_group = current.lock().group_id;
    let peer = Process::empty();
    let peer_pid = {
        let mut peer = peer.lock();
        peer.pid = ProcessID::new();
        peer.group_id = current_group;
        peer.parent = Some(current.clone());
        peer.pid.0 as i32
    };
    MANAGER
        .lock()
        .processes
        .insert(ProcessID(peer_pid as u64), peer.clone());

    expect_ok(
        SyscallArgs::new([peer_pid as u64, 0, 0, 0, 0, 0]).call::<Kill>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([peer_pid as u64, 65, 0, 0, 0, 0]).call::<Kill>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(
        SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<Kill>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([0, Signal::SIGUSR1 as u64, 0, 0, 0, 0]).call::<Kill>(),
        0,
    );
    assert!(
        current
            .lock()
            .pending_signals
            .contains(Signals::from(Signal::SIGUSR1))
    );
    assert!(
        peer.lock()
            .pending_signals
            .contains(Signals::from(Signal::SIGUSR1))
    );
    current
        .lock()
        .pending_signals
        .remove(Signals::from(Signal::SIGUSR1));
    peer.lock()
        .pending_signals
        .remove(Signals::from(Signal::SIGUSR1));

    send_signal_to_process_with_siginfo(
        &current,
        Signal::SIGUSR2,
        SigInfo::for_signal(Signal::SIGUSR2),
    );
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Pause>(),
        SyscallError::Interrupted,
    );
    {
        let mut current = current.lock();
        current
            .pending_signals
            .remove(Signals::from(Signal::SIGUSR2));
        current.pending_signal_info[Signal::SIGUSR2.index()] = None;
    }

    expect_ok(
        SyscallArgs::new([0, page + 192, 0, 0, 0, 0]).call::<Sigaltstack>(),
        0,
    );
    assert_eq!(
        read_user_value::<TestLinuxStack>(page + 192).ss_flags,
        SS_DISABLE
    );
    let altstack = TestLinuxStack {
        ss_sp: page + 4096,
        ss_flags: 0,
        ss_size: MINSIGSTKSZ,
    };
    write_user_value(page + 224, &altstack);
    expect_ok(
        SyscallArgs::new([page + 224, page + 256, 0, 0, 0, 0]).call::<Sigaltstack>(),
        0,
    );
    assert_eq!(
        read_user_value::<TestLinuxStack>(page + 256).ss_flags,
        SS_DISABLE
    );
    expect_ok(
        SyscallArgs::new([0, page + 288, 0, 0, 0, 0]).call::<Sigaltstack>(),
        0,
    );
    assert_eq!(
        read_user_value::<TestLinuxStack>(page + 288).ss_sp,
        altstack.ss_sp
    );
    assert_eq!(
        read_user_value::<TestLinuxStack>(page + 288).ss_size,
        MINSIGSTKSZ
    );
    write_user_value(
        page + 320,
        &TestLinuxStack {
            ss_sp: page + 8192,
            ss_flags: SS_DISABLE,
            ss_size: 9999,
        },
    );
    expect_ok(
        SyscallArgs::new([page + 320, 0, 0, 0, 0, 0]).call::<Sigaltstack>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([0, page + 352, 0, 0, 0, 0]).call::<Sigaltstack>(),
        0,
    );
    let disabled_stack = read_user_value::<TestLinuxStack>(page + 352);
    assert_eq!(disabled_stack.ss_flags, SS_DISABLE);
    assert_eq!(disabled_stack.ss_sp, 0);
    assert_eq!(disabled_stack.ss_size, 0);
    write_user_value(
        page + 384,
        &TestLinuxStack {
            ss_sp: page + 12288,
            ss_flags: SS_ONSTACK,
            ss_size: MINSIGSTKSZ,
        },
    );
    expect_errno(
        SyscallArgs::new([page + 384, 0, 0, 0, 0, 0]).call::<Sigaltstack>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(
        page + 416,
        &TestLinuxStack {
            ss_sp: 0,
            ss_flags: 0,
            ss_size: MINSIGSTKSZ,
        },
    );
    expect_errno(
        SyscallArgs::new([page + 416, 0, 0, 0, 0, 0]).call::<Sigaltstack>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(
        page + 448,
        &TestLinuxStack {
            ss_sp: page + 16384,
            ss_flags: 0,
            ss_size: MINSIGSTKSZ - 1,
        },
    );
    expect_errno(
        SyscallArgs::new([page + 448, 0, 0, 0, 0, 0]).call::<Sigaltstack>(),
        SyscallError::NoMemory,
    );

    extern "C" fn test_siginfo_handler(
        _: i32,
        _: *const SigInfo,
        _: *const crate::signal::UContext,
    ) {
    }
    let new_action = TestLinuxSigAction {
        handler: test_siginfo_handler as *const () as usize,
        flags: SA_SIGINFO,
        restorer: 0x1234_5678_9abc_def0usize,
        mask: Signal::SIGUSR1.mask(),
    };
    write_user_value(page + 480, &new_action);
    expect_ok(
        SyscallArgs::new([Signal::SIGUSR2 as u64, page + 480, page + 544, 8, 0, 0])
            .call::<RtSigaction>(),
        0,
    );
    let old_action = read_user_value::<TestLinuxSigAction>(page + 544);
    assert_eq!(old_action.handler, 0);
    assert_eq!(old_action.flags & SA_SIGINFO, 0);
    assert_eq!(old_action.mask, 0);
    expect_ok(
        SyscallArgs::new([Signal::SIGUSR2 as u64, 0, page + 576, 8, 0, 0]).call::<RtSigaction>(),
        0,
    );
    let installed_action = read_user_value::<TestLinuxSigAction>(page + 576);
    assert_eq!(
        installed_action.handler,
        test_siginfo_handler as *const () as usize
    );
    assert_ne!(installed_action.flags & SA_SIGINFO, 0);
    assert_eq!(installed_action.restorer, new_action.restorer);
    assert_eq!(installed_action.mask, Signal::SIGUSR1.mask());
    expect_errno(
        SyscallArgs::new([Signal::SIGUSR2 as u64, 1, 0, 8, 0, 0]).call::<RtSigaction>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([Signal::SIGUSR2 as u64, 0, 1, 8, 0, 0]).call::<RtSigaction>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([Signal::SIGUSR2 as u64, 0, 0, 4, 0, 0]).call::<RtSigaction>(),
        SyscallError::InvalidArguments,
    );

    let queued_process = Process::empty();
    let queued_pid = {
        let mut process = queued_process.lock();
        process.pid = ProcessID::new();
        process.parent = Some(current.clone());
        process.group_id = current_group;
        process.pid.0 as i32
    };
    MANAGER
        .lock()
        .processes
        .insert(ProcessID(queued_pid as u64), queued_process.clone());
    let mut queued_siginfo = SigInfo::for_process_signal(Signal::SIGTERM, 77, 88);
    queued_siginfo.si_code = SI_QUEUE;
    write_user_value(page + 768, &queued_siginfo);
    expect_ok(
        SyscallArgs::new([
            queued_pid as u64,
            Signal::SIGTERM as u64,
            page + 768,
            0,
            0,
            0,
        ])
        .call::<RtSigqueueinfo>(),
        0,
    );
    {
        let queued_process = queued_process.lock();
        assert!(
            queued_process
                .pending_signals
                .contains(Signals::from(Signal::SIGTERM))
        );
        let pending = queued_process.pending_signal_info[Signal::SIGTERM.index()]
            .expect("sigqueueinfo should store pending siginfo");
        assert_eq!(pending.si_code, SI_QUEUE);
        assert_eq!(pending.si_pid, 77);
        assert_eq!(pending.si_uid, 88);
    }
    expect_errno(
        SyscallArgs::new([0, Signal::SIGTERM as u64, 0, 0, 0, 0]).call::<RtSigqueueinfo>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([queued_pid as u64, 65, 0, 0, 0, 0]).call::<RtSigqueueinfo>(),
        SyscallError::InvalidArguments,
    );
    let tgkill_thread = crate::thread::thread::Thread::empty();
    let target_tid = {
        let mut thread = tgkill_thread.lock();
        thread.parent = current.clone();
        thread.id = crate::thread::misc::ThreadID::new();
        thread.id.0 as u64
    };
    crate::thread::THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .threads
        .insert(
            crate::thread::misc::ThreadID(target_tid),
            tgkill_thread.clone(),
        );
    current
        .lock()
        .threads
        .push(alloc::sync::Arc::downgrade(&tgkill_thread));
    let target_tgid = current.lock().pid.0 as u64;
    expect_ok(
        SyscallArgs::new([target_tgid, target_tid, Signal::SIGUSR1 as u64, 0, 0, 0])
            .call::<Tgkill>(),
        0,
    );
    {
        let thread = tgkill_thread.lock();
        assert!(
            thread
                .pending_signals
                .contains(Signals::from(Signal::SIGUSR1))
        );
        let pending = thread.pending_signal_info[Signal::SIGUSR1.index()]
            .expect("tgkill should queue thread siginfo");
        assert_eq!(pending.si_code, crate::misc::signal::SI_TKILL);
        assert_eq!(pending.si_pid, current.lock().pid.0 as i32);
        assert_eq!(pending.si_uid, current.lock().real_uid);
    }
    {
        let mut thread = tgkill_thread.lock();
        thread
            .pending_signals
            .remove(Signals::from(Signal::SIGUSR1));
        thread.pending_signal_info[Signal::SIGUSR1.index()] = None;
    }
    expect_errno(
        SyscallArgs::new([u64::MAX, target_tid, Signal::SIGUSR1 as u64, 0, 0, 0]).call::<Tgkill>(),
        SyscallError::NoProcess,
    );
    expect_errno(
        SyscallArgs::new([target_tgid, u64::MAX, Signal::SIGUSR1 as u64, 0, 0, 0]).call::<Tgkill>(),
        SyscallError::NoProcess,
    );
    expect_errno(
        SyscallArgs::new([target_tgid, target_tid, 65, 0, 0, 0]).call::<Tgkill>(),
        SyscallError::InvalidArguments,
    );
    crate::thread::THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .threads
        .remove(&crate::thread::misc::ThreadID(target_tid));
    current.lock().threads.retain(|candidate| {
        candidate
            .upgrade()
            .is_some_and(|thread| thread.lock().id.0 != target_tid)
    });

    {
        let thread_ref = crate::thread::get_current_thread();
        let mut thread = thread_ref.lock();
        thread.pending_signals = Signals::empty();
        thread.pending_signal_info.fill(None);
    }
    {
        let thread_parent = crate::thread::get_current_thread().lock().parent.clone();
        let mut current = thread_parent.lock();
        current.pending_signals = Signals::empty();
        current.pending_signal_info.fill(None);
    }

    let mut timed_siginfo = SigInfo::for_process_signal(Signal::SIGUSR1, 123, 456);
    timed_siginfo.si_code = SI_QUEUE;
    let thread_parent = crate::thread::get_current_thread().lock().parent.clone();
    send_signal_to_process_with_siginfo(&thread_parent, Signal::SIGUSR1, timed_siginfo);
    assert_eq!(
        crate::thread::get_current_thread()
            .lock()
            .pending_signals
            .bits(),
        0
    );
    assert_eq!(
        thread_parent.lock().pending_signals.bits(),
        Signal::SIGUSR1.mask()
    );
    send_signal_to_process_with_siginfo(&thread_parent, Signal::SIGUSR1, timed_siginfo);
    write_user_value(page + 608, &Signal::SIGUSR1.mask());
    expect_ok(
        SyscallArgs::new([page + 608, page + 640, 0, 8, 0, 0]).call::<RtSigtimedwait>(),
        Signal::SIGUSR1 as usize,
    );
    let waited_info = read_user_value::<TestWaitidSigInfo>(page + 640);
    assert_eq!(waited_info.si_signo, Signal::SIGUSR1 as i32);
    assert_eq!(waited_info.si_code, SI_QUEUE);
    assert_eq!(waited_info.si_pid, 123);
    assert_eq!(waited_info.si_uid, 456);
    assert!(
        !current
            .lock()
            .pending_signals
            .contains(Signals::from(Signal::SIGUSR1))
    );
    write_user_value(
        page + 736,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
    );
    expect_errno(
        SyscallArgs::new([page + 608, page + 640, page + 736, 8, 0, 0]).call::<RtSigtimedwait>(),
        SyscallError::TryAgain,
    );
    expect_errno(
        SyscallArgs::new([0, page + 640, 0, 8, 0, 0]).call::<RtSigtimedwait>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([page + 608, page + 640, page + 736, 4, 0, 0]).call::<RtSigtimedwait>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(
        page + 736,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        },
    );
    expect_errno(
        SyscallArgs::new([page + 608, page + 640, page + 736, 8, 0, 0]).call::<RtSigtimedwait>(),
        SyscallError::InvalidArguments,
    );

    {
        let mut current = thread.lock();
        current
            .pending_signals
            .insert(Signals::from(Signal::SIGUSR1));
        current
            .parent
            .lock()
            .pending_signals
            .insert(Signals::from(Signal::SIGTERM));
    }
    expect_ok(
        SyscallArgs::new([page + 168, 8, 0, 0, 0, 0]).call::<RtSigpending>(),
        0,
    );
    let pending = read_user_value::<u64>(page + 168);
    assert_ne!(pending & Signal::SIGUSR1.mask(), 0);
    assert_ne!(pending & Signal::SIGTERM.mask(), 0);
    expect_errno(
        SyscallArgs::new([0, 8, 0, 0, 0, 0]).call::<RtSigpending>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([page + 168, 4, 0, 0, 0, 0]).call::<RtSigpending>(),
        SyscallError::InvalidArguments,
    );
    {
        let mut current = thread.lock();
        current
            .pending_signals
            .remove(Signals::from(Signal::SIGUSR1));
        current
            .parent
            .lock()
            .pending_signals
            .remove(Signals::from(Signal::SIGTERM));
    }

    write_user_value(page + 160, &Signal::SIGUSR1.mask());
    expect_errno(
        SyscallArgs::new([page + 160, 4, 0, 0, 0, 0]).call::<RtSigsuspend>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, 8, 0, 0, 0, 0]).call::<RtSigsuspend>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([1, 8, 0, 0, 0, 0]).call::<RtSigsuspend>(),
        SyscallError::BadAddress,
    );
    MANAGER
        .lock()
        .processes
        .remove(&ProcessID(queued_pid as u64));
    MANAGER.lock().processes.remove(&ProcessID(peer_pid as u64));
    crate::thread::get_current_thread().lock().blocked_signals = saved_mask;
}

fn epoll_pwait2_syscalls_follow_linux_rules() {
    const EPOLLOUT: u32 = 0x004;

    let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let epoll_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<EpollCreate1>());
    let event = TestLinuxEpollEvent {
        events: EPOLLOUT,
        data: 0x1234_5678,
    };
    expect_ok(
        SyscallArgs::new([
            epoll_fd as u64,
            1,
            eventfd as u64,
            (&event as *const TestLinuxEpollEvent) as u64,
            0,
            0,
        ])
        .call::<EpollCtl>(),
        0,
    );

    let page = allocate_user_test_page();
    write_user_value(
        page,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 1,
        },
    );
    expect_ok(
        SyscallArgs::new([epoll_fd as u64, page + 64, 1, page, 0, 0]).call::<EpollPwait2>(),
        1,
    );
    let ready = read_user_value::<TestLinuxEpollEvent>(page + 64);
    let ready_events = ready.events;
    let ready_data = ready.data;
    assert_eq!(ready_events, EPOLLOUT);
    assert_eq!(ready_data, 0x1234_5678);

    write_user_value(
        page,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        },
    );
    expect_errno(
        SyscallArgs::new([epoll_fd as u64, page + 64, 1, page, 0, 0]).call::<EpollPwait2>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([epoll_fd as u64, page + 64, 0, 0, 0, 0]).call::<EpollPwait2>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([epoll_fd as u64, page + 64, 1, 1, 0, 0]).call::<EpollPwait2>(),
        SyscallError::BadAddress,
    );

    close_test_fd(epoll_fd);
    close_test_fd(eventfd);
}

fn object_control_syscalls_follow_linux_rules() {
    const TCGETS: u64 = 0x5401;
    const TIOCSPTLCK: u64 = 0x4004_5431;
    const TIOCGPTN: u64 = 0x8004_5430;
    const TIOCOUTQ: u64 = 0x5411;
    const FIONBIO: u64 = 0x5421;
    const FIOCLEX: u64 = 0x5451;
    const SOCK_STREAM: u64 = 1;
    const SOCK_RAW: u64 = 3;
    const AF_UNIX: u64 = 1;
    const AF_NETLINK: u64 = 16;
    const NETLINK_ROUTE: u64 = 0;
    const SCHED_OTHER: u64 = 0;
    const SCHED_FIFO: u64 = 1;

    assert_linux_layout::<LinuxTermios>(36, 4);
    assert_linux_layout::<TestLinuxSchedParam>(4, 4);

    let page = allocate_user_test_page();
    let [master_fd, slave_fd] = {
        write_user_value(page + 896, &0i32);
        write_user_value(page + 900, &0i32);
        expect_ok(
            SyscallArgs::new([page + 896, page + 900, 0, 0, 0, 0]).call::<CreatePty>(),
            0,
        );
        [
            read_user_value::<i32>(page + 896) as usize,
            read_user_value::<i32>(page + 900) as usize,
        ]
    };

    expect_ok(
        SyscallArgs::new([slave_fd as u64, TCGETS, page, 0, 0, 0]).call::<Ioctl>(),
        0,
    );
    let termios = read_user_value::<LinuxTermios>(page);
    assert_eq!(termios.c_cc.len(), 19);

    write_user_value(page + 128, &1i32);
    expect_ok(
        SyscallArgs::new([master_fd as u64, TIOCSPTLCK, page + 128, 0, 0, 0]).call::<Ioctl>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([master_fd as u64, TIOCSPTLCK, 1, 0, 0, 0]).call::<Ioctl>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([usize::MAX as u64, TCGETS, page, 0, 0, 0]).call::<Ioctl>(),
        SyscallError::BadFileDescriptor,
    );

    let unix_socket =
        expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
    assert_fd_flags(unix_socket, FdFlags::empty());
    write_user_value(page + 384, &1i32);
    expect_ok(
        SyscallArgs::new([unix_socket as u64, FIONBIO, page + 384, 0, 0, 0]).call::<Ioctl>(),
        0,
    );
    assert_object_flags(unix_socket, FileFlags::NONBLOCK);
    expect_ok(
        SyscallArgs::new([unix_socket as u64, FIOCLEX, 0, 0, 0, 0]).call::<Ioctl>(),
        0,
    );
    assert_fd_flags(unix_socket, FdFlags::CLOEXEC);
    expect_errno(
        SyscallArgs::new([unix_socket as u64, FIONBIO, 1, 0, 0, 0]).call::<Ioctl>(),
        SyscallError::BadAddress,
    );
    expect_ok(
        SyscallArgs::new([unix_socket as u64, TIOCOUTQ, page + 392, 0, 0, 0]).call::<Ioctl>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 392), 0);
    expect_errno(
        SyscallArgs::new([unix_socket as u64, TIOCGPTN, page + 392, 0, 0, 0]).call::<Ioctl>(),
        SyscallError::InappropriateIoctl,
    );

    let netlink_socket = expect_fd(
        SyscallArgs::new([AF_NETLINK, SOCK_RAW, NETLINK_ROUTE, 0, 0, 0]).call::<Socket>(),
    );
    expect_ok(
        SyscallArgs::new([netlink_socket as u64, FIOCLEX, 0, 0, 0, 0]).call::<Ioctl>(),
        0,
    );
    assert_fd_flags(netlink_socket, FdFlags::CLOEXEC);
    close_test_fd(netlink_socket);
    close_test_fd(unix_socket);

    write_user_value(page + 256, &TestLinuxSchedParam { sched_priority: 0 });
    expect_ok(
        SyscallArgs::new([0, SCHED_OTHER, page + 256, 0, 0, 0]).call::<SchedSetscheduler>(),
        0,
    );
    write_user_value(page + 260, &TestLinuxSchedParam { sched_priority: 1 });
    expect_ok(
        SyscallArgs::new([0, SCHED_FIFO, page + 260, 0, 0, 0]).call::<SchedSetscheduler>(),
        0,
    );
    write_user_value(page + 264, &TestLinuxSchedParam { sched_priority: 0 });
    expect_errno(
        SyscallArgs::new([0, SCHED_FIFO, page + 264, 0, 0, 0]).call::<SchedSetscheduler>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([u64::MAX, SCHED_OTHER, page + 256, 0, 0, 0]).call::<SchedSetscheduler>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, SCHED_OTHER, 0, 0, 0, 0]).call::<SchedSetscheduler>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([0, 99, page + 256, 0, 0, 0]).call::<SchedSetscheduler>(),
        SyscallError::InvalidArguments,
    );

    close_test_fd(master_fd);
    close_test_fd(slave_fd);
}

fn ptrace_syscalls_follow_linux_rules() {
    const PTRACE_TRACEME: u64 = 0;
    const PTRACE_SETOPTIONS: u64 = 0x4200;
    const PTRACE_GETEVENTMSG: u64 = 0x4201;
    const PTRACE_GETSIGINFO: u64 = 0x4202;
    const PTRACE_GETREGSET: u64 = 0x4204;
    const PTRACE_SEIZE: u64 = 0x4206;
    const PTRACE_GET_SYSCALL_INFO: u64 = 0x420e;
    const PTRACE_CONT: u64 = 7;
    const NT_PRSTATUS: u64 = 1;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestLinuxIovec {
        iov_base: *mut u8,
        iov_len: usize,
    }

    let current = get_current_process();
    let tracer_pid = current.lock().pid;
    let original_parent = current.lock().parent.clone();
    let original_ptrace = current.lock().ptrace;
    let parent = Process::empty();
    parent.lock().pid = ProcessID::new();
    current.lock().parent = Some(parent.clone());

    expect_ok(
        SyscallArgs::new([PTRACE_TRACEME, 0, 0, 0, 0, 0]).call::<Ptrace>(),
        0,
    );
    assert_eq!(current.lock().ptrace.tracer, Some(parent.lock().pid));
    expect_errno(
        SyscallArgs::new([PTRACE_TRACEME, 0, 0, 0, 0, 0]).call::<Ptrace>(),
        SyscallError::PermissionDenied,
    );

    let traced = Process::empty();
    let traced_pid = {
        let mut traced_locked = traced.lock();
        traced_locked.pid = ProcessID::new();
        traced_locked.parent = Some(current.clone());
        traced_locked.ptrace.tracer = Some(tracer_pid);
        traced_locked.ptrace.resume_mode = crate::process::ptrace::PtraceResumeMode::Stopped;
        traced_locked.ptrace.last_stop_status = ((Signal::SIGTRAP as i32) << 8) | 0x7f;
        traced_locked.wait_event = Some(crate::process::wait::ProcessWaitEvent::Stopped {
            status: (((Signal::SIGTRAP as i32) << 8) | 0x7f),
            ptrace: true,
        });
        traced_locked.pid.0
    };
    let traced_thread = crate::thread::thread::Thread::empty();
    {
        let mut thread = traced_thread.lock();
        thread.parent = traced.clone();
        thread.last_syscall_no = SyscallNumber::Read as u64;
        thread.last_user_snapshot.rax = -38;
        thread.last_user_snapshot.rip = 0x1234;
        thread.last_user_snapshot.rsp = 0x5678;
    }
    traced
        .lock()
        .threads
        .push(alloc::sync::Arc::downgrade(&traced_thread));
    MANAGER
        .lock()
        .processes
        .insert(ProcessID(traced_pid), traced.clone());
    crate::thread::THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .threads
        .insert(traced_thread.lock().id, traced_thread.clone());

    let page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([PTRACE_SETOPTIONS, traced_pid, 0, 1, 0, 0]).call::<Ptrace>(),
        0,
    );
    assert_eq!(traced.lock().ptrace.options, 1);

    expect_ok(
        SyscallArgs::new([PTRACE_GETEVENTMSG, traced_pid, 0, page, 0, 0]).call::<Ptrace>(),
        0,
    );
    assert_eq!(read_user_value::<usize>(page), 0);

    expect_ok(
        SyscallArgs::new([PTRACE_GETSIGINFO, traced_pid, 0, page + 64, 0, 0]).call::<Ptrace>(),
        0,
    );
    let siginfo = read_user_value::<SigInfo>(page + 64);
    assert_eq!(siginfo.si_signo, Signal::SIGTRAP as i32);

    let iov = TestLinuxIovec {
        iov_base: (page + 256) as *mut u8,
        iov_len: 216,
    };
    write_user_value(page + 192, &iov);
    expect_ok(
        SyscallArgs::new([PTRACE_GETREGSET, traced_pid, NT_PRSTATUS, page + 192, 0, 0])
            .call::<Ptrace>(),
        0,
    );
    assert_eq!(read_user_value::<TestLinuxIovec>(page + 192).iov_len, 216);

    traced.lock().ptrace.last_stop_kind = crate::process::ptrace::PtraceStopKind::SyscallExit;
    let copied = expect_fd(Ok(SyscallArgs::new([
        PTRACE_GET_SYSCALL_INFO,
        traced_pid,
        88,
        page + 512,
        0,
        0,
    ])
    .call::<Ptrace>()
    .expect("ptrace get syscall info should succeed")));
    assert_eq!(copied, 88);

    expect_ok(
        SyscallArgs::new([PTRACE_CONT, traced_pid, 0, 0, 0, 0]).call::<Ptrace>(),
        0,
    );
    assert_eq!(
        traced.lock().ptrace.resume_mode,
        crate::process::ptrace::PtraceResumeMode::Continue
    );

    let seize_target = Process::empty();
    let seize_pid = {
        let mut process = seize_target.lock();
        process.pid = ProcessID::new();
        process.pid.0 as i32
    };
    MANAGER
        .lock()
        .processes
        .insert(ProcessID(seize_pid as u64), seize_target.clone());
    expect_ok(
        SyscallArgs::new([PTRACE_SEIZE, seize_pid as u64, 0, 0, 0, 0]).call::<Ptrace>(),
        0,
    );
    assert_eq!(seize_target.lock().ptrace.tracer, Some(tracer_pid));
    expect_errno(
        SyscallArgs::new([PTRACE_CONT, seize_pid as u64, 0, 1, 0, 0]).call::<Ptrace>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([PTRACE_GETREGSET, traced_pid, 2, page + 192, 0, 0]).call::<Ptrace>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([9999, traced_pid, 0, 0, 0, 0]).call::<Ptrace>(),
        SyscallError::InvalidArguments,
    );

    current.lock().parent = original_parent;
    current.lock().ptrace = original_ptrace;
    MANAGER.lock().processes.remove(&ProcessID(traced_pid));
    MANAGER
        .lock()
        .processes
        .remove(&ProcessID(seize_pid as u64));
    crate::thread::THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .threads
        .remove(&traced_thread.lock().id);
}

fn mount_api_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const AT_RECURSIVE: u64 = 0x8000;
    const OPEN_TREE_CLOEXEC: u64 = 0x0008_0000;
    const MOVE_MOUNT_F_EMPTY_PATH: u64 = 0x0000_0004;
    const FSCONFIG_SET_STRING: u64 = 1;
    const FSCONFIG_CMD_CREATE: u64 = 6;
    const MS_BIND: u64 = 4096;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestLinuxMountAttr {
        attr_set: u64,
        attr_clr: u64,
        propagation: u64,
        userns_fd: u64,
    }

    let page = allocate_user_test_page();
    VirtualFS
        .lock()
        .create_dir(Path::new("/tmp/syscall-mount-test"))
        .unwrap();
    VirtualFS
        .lock()
        .create_dir(Path::new("/tmp/syscall-mount-test/src"))
        .unwrap();
    VirtualFS
        .lock()
        .create_dir(Path::new("/tmp/syscall-mount-test/dst"))
        .unwrap();
    VirtualFS
        .lock()
        .create_dir(Path::new("/tmp/syscall-mount-test/newdst"))
        .unwrap();
    write_user_cstr(page, b"/tmp/syscall-mount-test/src\0");
    write_user_cstr(page + 128, b"/tmp/syscall-mount-test/dst\0");
    write_user_cstr(page + 256, b"/tmp/syscall-mount-test/newdst\0");
    write_user_cstr(page + 384, b"tmpfs\0");
    write_user_cstr(page + 448, b"mode=700\0");
    write_user_cstr(page + 512, b"mode\0");
    write_user_cstr(page + 576, b"755\0");
    write_user_cstr(page + 704, b"\0");

    expect_ok(
        SyscallArgs::new([0, page + 128, page + 384, 0, page + 448, 0]).call::<Mount>(),
        0,
    );
    let mounted_root = {
        let mut vfs = VirtualFS.lock();
        vfs.open(Path::new("/tmp/syscall-mount-test/dst")).unwrap()
    };
    let mounted_stat = mounted_root.stat();
    assert_eq!(mounted_stat.st_mode & 0o777, 0o700);

    expect_ok(
        SyscallArgs::new([page, page + 128, 0, MS_BIND, 0, 0]).call::<Mount>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([page + 128, 0, 0, 0, 0, 0]).call::<Umount2>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([page + 128, 16, 0, 0, 0, 0]).call::<Umount2>(),
        SyscallError::InvalidArguments,
    );

    let fsfd = expect_fd(SyscallArgs::new([page + 384, 1, 0, 0, 0, 0]).call::<Fsopen>());
    assert_fd_flags(fsfd, FdFlags::CLOEXEC);
    expect_ok(
        SyscallArgs::new([
            fsfd as u64,
            FSCONFIG_SET_STRING,
            page + 512,
            page + 576,
            0,
            0,
        ])
        .call::<Fsconfig>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([fsfd as u64, FSCONFIG_CMD_CREATE, 0, 0, 0, 0]).call::<Fsconfig>(),
        0,
    );
    let mount_fd = expect_fd(SyscallArgs::new([fsfd as u64, 1, 0, 0, 0, 0]).call::<Fsmount>());
    assert_fd_flags(mount_fd, FdFlags::CLOEXEC);
    let mount_root_stat = get_object_current_process(mount_fd as u64)
        .unwrap()
        .as_statable()
        .unwrap()
        .stat();
    assert_eq!(mount_root_stat.st_mode & 0o777, 0o755);

    let tree_fd = expect_fd(
        SyscallArgs::new([AT_FDCWD, page + 128, OPEN_TREE_CLOEXEC, 0, 0, 0]).call::<OpenTree>(),
    );
    assert_fd_flags(tree_fd, FdFlags::CLOEXEC);
    expect_ok(
        SyscallArgs::new([
            mount_fd as u64,
            page + 704,
            AT_FDCWD,
            page + 256,
            MOVE_MOUNT_F_EMPTY_PATH,
            0,
        ])
        .call::<MoveMount>(),
        0,
    );
    let moved_root = {
        let mut vfs = VirtualFS.lock();
        vfs.open(Path::new("/tmp/syscall-mount-test/newdst"))
            .unwrap()
    };
    let moved_stat = moved_root.stat();
    assert_eq!(moved_stat.st_mode & 0o777, 0o755);

    write_user_value(
        page + 768,
        &TestLinuxMountAttr {
            attr_set: 1,
            attr_clr: 0,
            propagation: 0,
            userns_fd: 0,
        },
    );
    expect_ok(
        SyscallArgs::new([
            AT_FDCWD,
            page + 256,
            AT_RECURSIVE,
            page + 768,
            core::mem::size_of::<TestLinuxMountAttr>() as u64,
            0,
        ])
        .call::<MountSetattr>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([AT_FDCWD, 0, 0, page + 768, 1, 0]).call::<MountSetattr>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([
            AT_FDCWD,
            0,
            0,
            0,
            core::mem::size_of::<TestLinuxMountAttr>() as u64,
            0,
        ])
        .call::<MountSetattr>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([AT_FDCWD, page + 128, 2, 0, 0, 0]).call::<OpenTree>(),
        SyscallError::InvalidArguments,
    );

    close_test_fd(tree_fd);
    close_test_fd(mount_fd);
    close_test_fd(fsfd);
    let _ = SyscallArgs::new([page + 256, 0, 0, 0, 0, 0]).call::<Umount2>();
    let _ = VirtualFS
        .lock()
        .delete_file(Path::new("/tmp/syscall-mount-test/newdst"));
    let _ = VirtualFS
        .lock()
        .delete_file(Path::new("/tmp/syscall-mount-test/dst"));
    let _ = VirtualFS
        .lock()
        .delete_file(Path::new("/tmp/syscall-mount-test/src"));
    let _ = VirtualFS
        .lock()
        .delete_file(Path::new("/tmp/syscall-mount-test"));
}

fn process_and_signal_transition_helpers_follow_linux_rules() {
    let thread = crate::thread::get_current_thread();
    {
        let mut thread = thread.lock();
        thread.snapshot_state = crate::thread::misc::SnapshotState::SignalHandler;
        thread.blocked_signals = Signals::from(Signal::SIGTERM);
        thread
            .saved_blocked_signals
            .push(Signals::from(Signal::SIGUSR1));
        thread.restore_blocked_signals();
        assert_eq!(thread.blocked_signals.bits(), Signal::SIGUSR1.mask());
        thread.snapshot_state = crate::thread::misc::SnapshotState::Normal;
        thread.saved_blocked_signals.clear();
    }
}

fn clone_and_fork_syscalls_follow_linux_rules() {
    const SIGCHLD: u64 = 17;
    const CLONE_VM: u64 = 0x0000_0100;
    const CLONE_FS: u64 = 0x0000_0200;
    const CLONE_FILES: u64 = 0x0000_0400;
    const CLONE_PIDFD: u64 = 0x0000_1000;
    const CLONE_VFORK: u64 = 0x0000_4000;
    const CLONE_THREAD: u64 = 0x0001_0000;
    const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
    const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
    const CLONE_CHILD_SETTID: u64 = 0x0100_0000;

    assert_linux_layout::<TestLinuxCloneArgs>(88, 8);

    let page = allocate_user_test_page();

    write_user_value(page, &0i32);
    write_user_value(page + 8, &0i32);
    expect_errno(
        SyscallArgs::new([
            CLONE_PIDFD | CLONE_PARENT_SETTID | CLONE_CHILD_SETTID | CLONE_CHILD_CLEARTID | SIGCHLD,
            0,
            page,
            page + 8,
            0,
            0,
        ])
        .call::<Clone>(),
        SyscallError::NoSyscall,
    );

    expect_errno(
        SyscallArgs::new([CLONE_VFORK | SIGCHLD, 0, 0, 0, 0, 0]).call::<Clone>(),
        SyscallError::NoSyscall,
    );
    expect_errno(
        SyscallArgs::new([CLONE_VM | SIGCHLD, 0, 0, 0, 0, 0]).call::<Clone>(),
        SyscallError::NoSyscall,
    );
    expect_errno(
        SyscallArgs::new([CLONE_PIDFD | SIGCHLD, 0, 0, 0, 0, 0]).call::<Clone>(),
        SyscallError::BadAddress,
    );

    expect_errno(
        SyscallArgs::new([
            CLONE_THREAD | CLONE_VM | CLONE_FS | CLONE_FILES,
            0,
            0,
            0,
            0,
            0,
        ])
        .call::<Clone>(),
        SyscallError::NoSyscall,
    );

    write_user_value(
        page + 256,
        &TestLinuxCloneArgs {
            set_tid: 1,
            ..Default::default()
        },
    );
    expect_errno(
        SyscallArgs::new([
            page + 256,
            core::mem::size_of::<TestLinuxCloneArgs>() as u64,
            0,
            0,
            0,
            0,
        ])
        .call::<Clone3>(),
        SyscallError::NoSyscall,
    );
    expect_errno(
        SyscallArgs::new([
            0,
            core::mem::size_of::<TestLinuxCloneArgs>() as u64,
            0,
            0,
            0,
            0,
        ])
        .call::<Clone3>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([page + 256, 8, 0, 0, 0, 0]).call::<Clone3>(),
        SyscallError::InvalidArguments,
    );

    write_user_value(
        page + 288,
        &TestLinuxCloneArgs {
            flags: CLONE_PIDFD | CLONE_PARENT_SETTID,
            pidfd: page + 320,
            parent_tid: page + 320,
            exit_signal: SIGCHLD,
            ..Default::default()
        },
    );
    expect_errno(
        SyscallArgs::new([
            page + 288,
            core::mem::size_of::<TestLinuxCloneArgs>() as u64,
            0,
            0,
            0,
            0,
        ])
        .call::<Clone3>(),
        SyscallError::NoSyscall,
    );

    write_user_value(
        page + 384,
        &TestLinuxCloneArgs {
            flags: CLONE_PIDFD,
            exit_signal: SIGCHLD,
            ..Default::default()
        },
    );
    expect_errno(
        SyscallArgs::new([
            page + 384,
            core::mem::size_of::<TestLinuxCloneArgs>() as u64,
            0,
            0,
            0,
            0,
        ])
        .call::<Clone3>(),
        SyscallError::InvalidArguments,
    );
}

fn futex_syscalls_follow_linux_rules() {
    const FUTEX_WAIT: u64 = 0;
    const FUTEX_WAKE: u64 = 1;
    const FUTEX_WAIT_BITSET: u64 = 9;
    const FUTEX_WAKE_BITSET: u64 = 10;

    let page = allocate_user_test_page();
    write_user_value(page + 384, &7u32);
    expect_errno(
        SyscallArgs::new([page + 384, FUTEX_WAIT, 8, 0, 0, 0]).call::<Futex>(),
        SyscallError::TryAgain,
    );
    write_user_value(
        page + 392,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        },
    );
    expect_errno(
        SyscallArgs::new([page + 384, FUTEX_WAIT, 7, page + 392, 0, 0]).call::<Futex>(),
        SyscallError::InvalidArguments,
    );
    expect_ok(
        SyscallArgs::new([page + 384, FUTEX_WAKE, 3, 0, 0, 0]).call::<Futex>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([page + 384, FUTEX_WAIT_BITSET, 7, 0, 0, 0]).call::<Futex>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([page + 384, FUTEX_WAKE_BITSET, 1, 0, 0, 0]).call::<Futex>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(
        page + 392,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
    );
    expect_errno(
        SyscallArgs::new([page + 384, FUTEX_WAIT_BITSET, 7, page + 392, 0, 1]).call::<Futex>(),
        SyscallError::TimedOut,
    );
    expect_ok(
        SyscallArgs::new([page + 384, FUTEX_WAKE_BITSET, 1, 0, 0, 1]).call::<Futex>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([0, FUTEX_WAKE, 1, 0, 0, 0]).call::<Futex>(),
        SyscallError::BadAddress,
    );
}

fn execve_syscalls_follow_linux_rules() {
    let page = allocate_user_test_page();

    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Execve>(),
        SyscallError::BadAddress,
    );
    write_user_cstr(page + 512, b"/does-not-exist\0");
    let argv = [page + 512, 0];
    let envp = [0u64];
    write_user_value(page + 640, &argv);
    write_user_value(page + 704, &envp);
    expect_errno(
        SyscallArgs::new([page + 512, page + 640, page + 704, 0, 0, 0]).call::<Execve>(),
        SyscallError::FileNotFound,
    );
}

fn exit_thread_semantics_follow_linux_rules() {
    let saved_process_ref = get_current_process();
    let page = allocate_user_test_page();

    write_user_value(page + 448, &99i32);
    let (helper_process, helper_thread) = Process::fork(saved_process_ref.clone());
    let helper_pid = helper_process.lock().pid;
    MANAGER
        .lock()
        .processes
        .insert(helper_pid, helper_process.clone());
    helper_thread.lock().clear_child_tid = page + 448;
    helper_process.lock().exit_status = Some(ProcessExitStatus::Exited(12));
    let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
    thread_manager.mark_thread_exited(helper_thread.clone());
    thread_manager.cleanup_exited_threads();
    drop(thread_manager);
    assert_eq!(
        helper_process
            .lock()
            .addrspace
            .read::<i32>((page + 448) as *const i32)
            .expect("child clear_child_tid should be zeroed"),
        0
    );
    MANAGER.lock().processes.remove(&helper_pid);
}

fn exit_group_semantics_follow_linux_rules() {
    let exit_group_process = Process::empty();
    exit_group_process.lock().pid = ProcessID::new();
    let terminated_threads = exit_group_process
        .lock()
        .terminate_inner(ProcessExitStatus::from_exit_code(23));
    assert_eq!(
        exit_group_process.lock().exit_status,
        Some(ProcessExitStatus::Exited(23))
    );
    assert!(terminated_threads.is_empty());
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

fn pselect6_syscalls_follow_linux_rules() {
    let page = allocate_user_test_page();
    let thread = crate::thread::get_current_thread();
    let saved_mask = thread.lock().blocked_signals;

    write_user_value(
        page,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
    );
    let new_mask = Signal::SIGUSR1.mask();
    write_user_value(page + 32, &new_mask);
    write_user_value(
        page + 64,
        &TestLinuxSigSetArg {
            sigmask: page + 32,
            sigsetsize: 8,
        },
    );
    expect_ok(
        SyscallArgs::new([0, 0, 0, 0, page, page + 64]).call::<Pselect6>(),
        0,
    );
    assert_eq!(thread.lock().blocked_signals.bits(), saved_mask.bits());

    let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let readfds = [0u64; 1];
    let mut writefds = [0u64; 1];
    unsafe {
        fdset_insert(writefds.as_mut_ptr(), eventfd);
    }
    write_user_value(page + 96, &readfds);
    write_user_value(page + 104, &writefds);
    write_user_value(page + 112, &[0u64; 1]);
    expect_ok(
        SyscallArgs::new([eventfd as u64 + 1, page + 96, page + 104, page + 112, 0, 0])
            .call::<Pselect6>(),
        1,
    );
    assert_eq!(read_user_value::<u64>(page + 96), 0);
    assert_eq!(read_user_value::<u64>(page + 104), 1u64 << eventfd);
    assert_eq!(read_user_value::<u64>(page + 112), 0);

    write_user_value(
        page + 120,
        &TestLinuxSigSetArg {
            sigmask: page + 32,
            sigsetsize: 4,
        },
    );
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, page + 120]).call::<Pselect6>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(
        page + 120,
        &TestLinuxSigSetArg {
            sigmask: 1,
            sigsetsize: 8,
        },
    );
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, 0, page + 120]).call::<Pselect6>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<Pselect6>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(
        page,
        &TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        },
    );
    expect_errno(
        SyscallArgs::new([0, 0, 0, 0, page, 0]).call::<Pselect6>(),
        SyscallError::InvalidArguments,
    );

    close_test_fd(eventfd);
    crate::thread::get_current_thread().lock().blocked_signals = saved_mask;
}

fn memory_mapping_syscalls_follow_linux_rules() {
    const MAP_SHARED: u64 = 0x01;
    const MAP_PRIVATE: u64 = 0x02;
    const MAP_ANONYMOUS: u64 = 0x20;
    const MAP_FIXED_NOREPLACE: u64 = 0x100000;
    const MREMAP_MAYMOVE: u64 = 0x1;
    const MS_ASYNC: u64 = 0x1;
    const MS_INVALIDATE: u64 = 0x2;
    const MS_SYNC: u64 = 0x4;
    const AT_FDCWD: u64 = (-100i32) as u64;

    let process = get_current_process();
    let original_break = process.lock().program_break;
    let current_break = SyscallArgs::new([0, 0, 0, 0, 0, 0])
        .call::<Brk>()
        .expect("brk query should succeed") as u64;
    let grown_break = current_break + 5000;
    expect_ok(
        SyscallArgs::new([grown_break, 0, 0, 0, 0, 0]).call::<Brk>(),
        grown_break as usize,
    );
    assert_eq!(process.lock().program_break, grown_break);
    let brk_area = process
        .lock()
        .addrspace
        .get_area(x86_64::VirtAddr::new(current_break.div_ceil(4096) * 4096))
        .cloned()
        .expect("brk growth should create mapped area");
    assert!(matches!(brk_area.data, Data::Normal));
    expect_ok(
        SyscallArgs::new([current_break, 0, 0, 0, 0, 0]).call::<Brk>(),
        current_break as usize,
    );
    process.lock().program_break = original_break;

    let anon_addr = SyscallArgs::new([
        0,
        8192,
        (Protection::READ | Protection::WRITE).bits() as u64,
        MAP_PRIVATE | MAP_ANONYMOUS,
        u64::MAX,
        0,
    ])
    .call::<Mmap>()
    .expect("anon mmap should succeed") as u64;
    let anon_area = process
        .lock()
        .addrspace
        .get_area(x86_64::VirtAddr::new(anon_addr))
        .cloned()
        .expect("anon mmap should register area");
    assert!(matches!(anon_area.data, Data::Normal));
    assert_eq!(
        SyscallArgs::new([anon_addr, 4096, 0, 0, 0, 0]).call::<Mlock>(),
        Ok(0),
        "mlock should succeed on initial anonymous mapping"
    );
    expect_ok(
        SyscallArgs::new([anon_addr, 4096, 0, 0, 0, 0]).call::<Munlock>(),
        0,
    );
    process
        .lock()
        .addrspace
        .write_buffer(anon_addr as *mut u8, b"mmap")
        .unwrap();
    assert_user_bytes(anon_addr, b"mmap");
    expect_errno(
        SyscallArgs::new([
            0,
            0,
            Protection::READ.bits() as u64,
            MAP_PRIVATE | MAP_ANONYMOUS,
            u64::MAX,
            0,
        ])
        .call::<Mmap>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([
            0x2000,
            4096,
            Protection::READ.bits() as u64,
            MAP_PRIVATE | MAP_ANONYMOUS,
            u64::MAX,
            0,
        ])
        .call::<Mmap>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([
            anon_addr,
            4096,
            Protection::READ.bits() as u64,
            MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE,
            u64::MAX,
            0,
        ])
        .call::<Mmap>(),
        SyscallError::FileAlreadyExists,
    );

    expect_ok(
        SyscallArgs::new([anon_addr, 4096, Protection::READ.bits() as u64, 0, 0, 0])
            .call::<Mprotect>(),
        0,
    );
    let readonly_area = process
        .lock()
        .addrspace
        .get_area(x86_64::VirtAddr::new(anon_addr))
        .cloned()
        .expect("mprotect should keep mapping");
    assert!(
        !readonly_area
            .flags
            .contains(x86_64::structures::paging::PageTableFlags::WRITABLE)
    );

    let remapped_addr = SyscallArgs::new([anon_addr, 4096, 8192, MREMAP_MAYMOVE, 0, 0])
        .call::<Mremap>()
        .expect("mremap should succeed") as u64;
    assert_user_bytes(remapped_addr, b"mmap");
    assert!(
        process
            .lock()
            .addrspace
            .get_area(x86_64::VirtAddr::new(anon_addr))
            .is_none()
    );
    expect_errno(
        SyscallArgs::new([remapped_addr, 4096, 12288, 0, 0, 0]).call::<Mremap>(),
        SyscallError::NoMemory,
    );
    expect_ok(
        SyscallArgs::new([anon_addr, 0, 0, 0, 0, 0]).call::<Mlock>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([anon_addr, 0, 0, 0, 0, 0]).call::<Munlock>(),
        0,
    );
    assert_eq!(
        SyscallArgs::new([remapped_addr + 1, 4095, 0, 0, 0, 0]).call::<Mlock>(),
        Ok(0),
        "mlock should succeed on unaligned remapped address"
    );
    expect_ok(
        SyscallArgs::new([remapped_addr + 1, 4095, 0, 0, 0, 0]).call::<Munlock>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([anon_addr, 4096, 0, 0, 0, 0]).call::<Mlock>(),
        SyscallError::NoMemory,
    );
    expect_errno(
        SyscallArgs::new([anon_addr, 4096, 0, 0, 0, 0]).call::<Munlock>(),
        SyscallError::NoMemory,
    );
    expect_errno(
        SyscallArgs::new([0x2000_0000, 4096, 0, 0, 0, 0]).call::<Mlock>(),
        SyscallError::NoMemory,
    );
    expect_errno(
        SyscallArgs::new([0x2000_0000, 4096, 0, 0, 0, 0]).call::<Munlock>(),
        SyscallError::NoMemory,
    );
    let old_memlock_limit = process.lock().rlimit_memlock_cur;
    process.lock().rlimit_memlock_cur = 0;
    expect_errno(
        SyscallArgs::new([remapped_addr, 4096, 0, 0, 0, 0]).call::<Mlock>(),
        SyscallError::NoMemory,
    );
    process.lock().rlimit_memlock_cur = old_memlock_limit;

    let mincore_vec = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([remapped_addr, 4096, mincore_vec, 0, 0, 0]).call::<Mincore>(),
        0,
    );
    assert_ne!(read_user_value::<u8>(mincore_vec), 0);
    expect_errno(
        SyscallArgs::new([remapped_addr + 1, 4096, mincore_vec, 0, 0, 0]).call::<Mincore>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([remapped_addr, 4096, 0, 0, 0, 0]).call::<Mincore>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([0x2000_0000, 4096, mincore_vec, 0, 0, 0]).call::<Mincore>(),
        SyscallError::NoMemory,
    );

    let page = allocate_user_test_page();
    write_user_cstr(page, b"/tmp/syscall-mmap-file-test\0");
    let fd = expect_fd(
        SyscallArgs::new([
            AT_FDCWD,
            page,
            (OpenFlags::CREAT | OpenFlags::TRUNC).bits() as u64,
            0o600,
            0,
            0,
        ])
        .call::<OpenAt>(),
    );
    process
        .lock()
        .addrspace
        .write_buffer((page + 128) as *mut u8, b"abcdef")
        .unwrap();
    expect_ok(
        SyscallArgs::new([fd as u64, page + 128, 6, 0, 0, 0]).call::<Write>(),
        6,
    );
    let file_map_addr = SyscallArgs::new([
        0,
        4096,
        (Protection::READ | Protection::WRITE).bits() as u64,
        MAP_SHARED,
        fd as u64,
        0,
    ])
    .call::<Mmap>()
    .expect("file mmap should succeed") as u64;
    process
        .lock()
        .addrspace
        .write_buffer(file_map_addr as *mut u8, b"XYZ")
        .unwrap();
    expect_ok(
        SyscallArgs::new([file_map_addr, 4096, MS_SYNC, 0, 0, 0]).call::<Msync>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    process
        .lock()
        .addrspace
        .write_buffer((page + 256) as *mut u8, &[0; 6])
        .unwrap();
    expect_ok(
        SyscallArgs::new([fd as u64, page + 256, 6, 0, 0, 0]).call::<Read>(),
        6,
    );
    assert_user_bytes(page + 256, b"XYZdef");
    expect_ok(
        SyscallArgs::new([file_map_addr, 0, 0, 0, 0, 0]).call::<Msync>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([file_map_addr + 1, 4096, MS_SYNC, 0, 0, 0]).call::<Msync>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([file_map_addr, 4096, MS_ASYNC | MS_SYNC, 0, 0, 0]).call::<Msync>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([file_map_addr, 4096, MS_INVALIDATE, 0, 0, 0]).call::<Msync>(),
        SyscallError::OperationNotSupported,
    );

    expect_ok(
        SyscallArgs::new([file_map_addr, 4096, 0, 0, 0, 0]).call::<Munmap>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([remapped_addr, 8192, 0, 0, 0, 0]).call::<Munmap>(),
        0,
    );
    assert!(
        process
            .lock()
            .addrspace
            .get_area(x86_64::VirtAddr::new(remapped_addr))
            .is_none()
    );
    close_test_fd(fd);
    let _ = VirtualFS
        .lock()
        .delete_file(Path::new("/tmp/syscall-mmap-file-test"));
}

fn sysv_shm_syscalls_follow_linux_rules() {
    const IPC_PRIVATE: u64 = 0;
    const IPC_CREAT: u64 = 0o1000;
    const IPC_EXCL: u64 = 0o2000;
    const IPC_RMID: u64 = 0;
    const IPC_STAT: u64 = 2;
    const SHM_RDONLY: u64 = 0o10000;
    const SHM_RND: u64 = 0o20000;

    let key = 0x55aa_u64;
    let shmid = SyscallArgs::new([key, 4097, IPC_CREAT | IPC_EXCL | 0o600, 0, 0, 0])
        .call::<Shmget>()
        .expect("shmget should create segment") as u64;
    expect_ok(
        SyscallArgs::new([key, 4096, IPC_CREAT, 0, 0, 0]).call::<Shmget>(),
        shmid as usize,
    );
    expect_errno(
        SyscallArgs::new([key, 4096, IPC_CREAT | IPC_EXCL, 0, 0, 0]).call::<Shmget>(),
        SyscallError::FileAlreadyExists,
    );
    expect_errno(
        SyscallArgs::new([key, 8192, IPC_CREAT, 0, 0, 0]).call::<Shmget>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0xdead, 4096, 0, 0, 0, 0]).call::<Shmget>(),
        SyscallError::FileNotFound,
    );
    expect_errno(
        SyscallArgs::new([IPC_PRIVATE, 0, IPC_CREAT, 0, 0, 0]).call::<Shmget>(),
        SyscallError::InvalidArguments,
    );

    let attach_addr = SyscallArgs::new([shmid, 0, 0, 0, 0, 0])
        .call::<Shmat>()
        .expect("shmat should attach") as u64;
    get_current_process()
        .lock()
        .addrspace
        .write_buffer(attach_addr as *mut u8, b"shm!")
        .unwrap();
    assert_user_bytes(attach_addr, b"shm!");

    let stat_page = allocate_user_test_page();
    expect_ok(
        SyscallArgs::new([shmid, IPC_STAT, stat_page, 0, 0, 0]).call::<Shmctl>(),
        0,
    );
    let stat = read_user_value::<LinuxShmidDs>(stat_page);
    assert_eq!(stat.shm_perm.__ipc_perm_key, key as i32);
    assert_eq!(stat.shm_perm.mode & 0o777, 0o600);
    assert_eq!(stat.shm_segsz, 4097);
    assert_eq!(stat.shm_nattch, 1);

    expect_errno(
        SyscallArgs::new([shmid, IPC_STAT, 0, 0, 0, 0]).call::<Shmctl>(),
        SyscallError::BadAddress,
    );
    expect_errno(
        SyscallArgs::new([shmid, 99, 0, 0, 0, 0]).call::<Shmctl>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([shmid, 0, SHM_RDONLY | 0x8, 0, 0, 0]).call::<Shmat>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([shmid, 123, SHM_RDONLY, 0, 0, 0]).call::<Shmat>(),
        SyscallError::InvalidArguments,
    );

    let rounded_addr = SyscallArgs::new([shmid, 0x12345, SHM_RDONLY | SHM_RND, 0, 0, 0])
        .call::<Shmat>()
        .expect("shmat with SHM_RND should round address") as u64;
    assert_eq!(rounded_addr, 0x12000);
    let readonly_area = get_current_process()
        .lock()
        .addrspace
        .get_area(x86_64::VirtAddr::new(rounded_addr))
        .cloned()
        .expect("readonly shm attach should create area");
    assert!(
        !readonly_area
            .flags
            .contains(x86_64::structures::paging::PageTableFlags::WRITABLE)
    );

    expect_ok(
        SyscallArgs::new([shmid, IPC_RMID, 0, 0, 0, 0]).call::<Shmctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([rounded_addr, 0, 0, 0, 0, 0]).call::<Shmdt>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([attach_addr, 0, 0, 0, 0, 0]).call::<Shmdt>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([attach_addr, 0, 0, 0, 0, 0]).call::<Shmdt>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([shmid, IPC_STAT, stat_page, 0, 0, 0]).call::<Shmctl>(),
        SyscallError::InvalidArguments,
    );
}

fn key_and_bpf_syscalls_follow_linux_rules() {
    const KEY_SPEC_SESSION_KEYRING: u64 = (-3i32) as u64;
    const KEY_SPEC_USER_KEYRING: u64 = (-4i32) as u64;
    const BPF_MAP_CREATE: u64 = 0;
    const BPF_MAP_LOOKUP_ELEM: u64 = 1;
    const BPF_MAP_UPDATE_ELEM: u64 = 2;
    const BPF_PROG_LOAD: u64 = 5;
    const BPF_PROG_ATTACH: u64 = 8;
    const BPF_PROG_DETACH: u64 = 9;
    const BPF_MAP_TYPE_ARRAY: u32 = 2;

    let page = allocate_user_test_page();
    write_user_cstr(page, b"user\0");
    write_user_cstr(page + 64, b"demo\0");
    expect_errno(
        SyscallArgs::new([page, page + 64, 1, 1, KEY_SPEC_SESSION_KEYRING, 0]).call::<AddKey>(),
        SyscallError::BadAddress,
    );
    let key_serial = SyscallArgs::new([page, page + 64, 0, 0, KEY_SPEC_SESSION_KEYRING, 0])
        .call::<AddKey>()
        .expect("add_key should create key") as u64;
    let session_keyring = SyscallArgs::new([0, KEY_SPEC_SESSION_KEYRING, 0, 0, 0, 0])
        .call::<Keyctl>()
        .expect("get_keyring_id should create session keyring") as u64;
    assert_eq!(
        SyscallArgs::new([1, 0, 0, 0, 0, 0])
            .call::<Keyctl>()
            .expect("join_session_keyring should return current session keyring") as u64,
        session_keyring
    );
    expect_ok(
        SyscallArgs::new([5, session_keyring, 0x1234_5678, 0, 0, 0]).call::<Keyctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([8, key_serial, session_keyring, 0, 0, 0]).call::<Keyctl>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([3, key_serial, 0, 0, 0, 0]).call::<Keyctl>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([8, key_serial, session_keyring, 0, 0, 0]).call::<Keyctl>(),
        SyscallError::InvalidArguments,
    );
    expect_errno(
        SyscallArgs::new([0, KEY_SPEC_USER_KEYRING, 0, 0, 0, 0]).call::<Keyctl>(),
        SyscallError::NoData,
    );
    expect_errno(
        SyscallArgs::new([99, 0, 0, 0, 0, 0]).call::<Keyctl>(),
        SyscallError::NoSyscall,
    );

    expect_errno(
        SyscallArgs::new([BPF_MAP_CREATE, 0, 0, 0, 0, 0]).call::<Bpf>(),
        SyscallError::BadAddress,
    );
    let mut create_attr = TestBpfMapCreateAttr {
        map_type: BPF_MAP_TYPE_ARRAY,
        key_size: 0,
        value_size: 4,
        max_entries: 2,
        ..Default::default()
    };
    write_user_value(page + 128, &create_attr);
    expect_errno(
        SyscallArgs::new([
            BPF_MAP_CREATE,
            page + 128,
            core::mem::size_of::<TestBpfMapCreateAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
        SyscallError::InvalidArguments,
    );
    create_attr.key_size = 4;
    write_user_value(page + 128, &create_attr);
    let map_fd = expect_fd(
        SyscallArgs::new([
            BPF_MAP_CREATE,
            page + 128,
            core::mem::size_of::<TestBpfMapCreateAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
    );

    write_user_value(page + 256, &0u32);
    write_user_value(page + 264, &0x1122_3344u32);
    let elem_attr = TestBpfMapElemAttr {
        map_fd: map_fd as u32,
        key: page + 256,
        value: page + 264,
        flags: 0,
        ..Default::default()
    };
    write_user_value(page + 272, &elem_attr);
    expect_ok(
        SyscallArgs::new([
            BPF_MAP_UPDATE_ELEM,
            page + 272,
            core::mem::size_of::<TestBpfMapElemAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
        0,
    );
    write_user_value(page + 320, &0u32);
    let lookup_attr = TestBpfMapElemAttr {
        map_fd: map_fd as u32,
        key: page + 256,
        value: page + 320,
        flags: 0,
        ..Default::default()
    };
    write_user_value(page + 328, &lookup_attr);
    expect_ok(
        SyscallArgs::new([
            BPF_MAP_LOOKUP_ELEM,
            page + 328,
            core::mem::size_of::<TestBpfMapElemAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
        0,
    );
    assert_eq!(read_user_value::<u32>(page + 320), 0x1122_3344);

    write_user_value(page + 256, &9u32);
    expect_errno(
        SyscallArgs::new([
            BPF_MAP_LOOKUP_ELEM,
            page + 328,
            core::mem::size_of::<TestBpfMapElemAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
        SyscallError::FileNotFound,
    );

    let bad_prog = TestBpfProgLoadAttr {
        prog_type: 0,
        insn_cnt: 0,
        ..Default::default()
    };
    write_user_value(page + 384, &bad_prog);
    expect_errno(
        SyscallArgs::new([
            BPF_PROG_LOAD,
            page + 384,
            core::mem::size_of::<TestBpfProgLoadAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
        SyscallError::InvalidArguments,
    );
    write_user_value(page + 448, &[0u8; 8]);
    write_user_cstr(page + 512, b"GPL\0");
    let prog = TestBpfProgLoadAttr {
        prog_type: 1,
        insn_cnt: 1,
        insns: page + 448,
        license: page + 512,
        ..Default::default()
    };
    write_user_value(page + 384, &prog);
    let prog_fd = expect_fd(
        SyscallArgs::new([
            BPF_PROG_LOAD,
            page + 384,
            core::mem::size_of::<TestBpfProgLoadAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
    );
    let target_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let attach_attr = TestBpfProgAttachAttr {
        target_fd: target_fd as u32,
        attach_bpf_fd: prog_fd as u32,
        ..Default::default()
    };
    write_user_value(page + 576, &attach_attr);
    expect_ok(
        SyscallArgs::new([
            BPF_PROG_ATTACH,
            page + 576,
            core::mem::size_of::<TestBpfProgAttachAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
        0,
    );
    expect_ok(
        SyscallArgs::new([
            BPF_PROG_DETACH,
            page + 576,
            core::mem::size_of::<TestBpfProgAttachAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
        0,
    );
    expect_errno(
        SyscallArgs::new([
            99,
            page + 576,
            core::mem::size_of::<TestBpfProgAttachAttr>() as u64,
            0,
            0,
            0,
        ])
        .call::<Bpf>(),
        SyscallError::InvalidArguments,
    );

    close_test_fd(target_fd);
    close_test_fd(prog_fd);
    close_test_fd(map_fd);
}
