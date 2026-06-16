use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::{
    filesystem::info::LinuxStat,
    filesystem::{
        absolute_path::AbsolutePath, object::mount_device_id_for_path, path::Path, vfs::VirtualFS,
    },
    ipc::sysv_shm::LinuxShmidDs,
    memory::{addrspace::mem_area::Data, protection::Protection},
    misc::{signal::send_signal_to_process_with_siginfo, timer::ClockId},
    object::{FileFlags, config::LinuxTermios, misc::get_object_current_process, traits::Statable},
    polling::{event::PollableEvent, object::Pollable},
    process::{
        ControllingTerminal, FdFlags, Process, ProcessExitStatus,
        group::{ProcessGroupID, SessionID},
        manager::{MANAGER, get_current_process, terminate_process},
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
            QuotactlFd, Read, Readlink, ReadlinkAt, Reboot, Recvfrom, Recvmsg, Removexattr, Rename,
            RenameAt, RenameAt2, Rmdir, Rseq, RtSigaction, RtSigpending, RtSigprocmask,
            RtSigqueueinfo, RtSigsuspend, RtSigtimedwait, SchedGetPriorityMax, SchedGetPriorityMin,
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
    thread::{THREAD_MANAGER, extended_state::active_user_extended_state_ptr},
};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct LinuxDirent64Header {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxStatxTimestamp {
    tv_sec: i64,
    tv_nsec: u32,
    __reserved: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxStatx {
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
pub(crate) struct TestLinuxFileHandle {
    handle_bytes: u32,
    handle_type: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(dead_code)]
pub(crate) struct TestLinuxPollFd {
    pub(crate) fd: i32,
    pub(crate) events: i16,
    pub(crate) revents: i16,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxEpollEvent {
    pub(crate) events: u32,
    pub(crate) data: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TestLinuxSockAddrUn {
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
pub(crate) struct TestLinuxSockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestRelibcIovec {
    iov_base: *mut u8,
    iov_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestRelibcMsgHdr {
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
pub(crate) struct TestRelibcMmsghdr {
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
pub(crate) struct TestLinuxUcred {
    pid: i32,
    uid: u32,
    gid: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestRightsControlMessage {
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
    typed_syscall_arg_conversion,
    "typed syscall args convert flags and enums at boundary",
    typed_syscall_args_convert_flags_and_enums_at_boundary
);

pub(crate) fn syscall_number_lookup_matches_x86_64_abi_values() {
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
pub(crate) struct TestLinuxTimeval {
    pub(crate) tv_sec: i64,
    pub(crate) tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxTimezone {
    tz_minuteswest: i32,
    tz_dsttime: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxRusage {
    pub(crate) ru_utime: TestLinuxTimeval,
    pub(crate) ru_stime: TestLinuxTimeval,
    pub(crate) ru_maxrss: i64,
    pub(crate) ru_ixrss: i64,
    pub(crate) ru_idrss: i64,
    pub(crate) ru_isrss: i64,
    pub(crate) ru_minflt: i64,
    pub(crate) ru_majflt: i64,
    pub(crate) ru_nswap: i64,
    pub(crate) ru_inblock: i64,
    pub(crate) ru_oublock: i64,
    pub(crate) ru_msgsnd: i64,
    pub(crate) ru_msgrcv: i64,
    pub(crate) ru_nsignals: i64,
    pub(crate) ru_nvcsw: i64,
    pub(crate) ru_nivcsw: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxSchedParam {
    pub(crate) sched_priority: i32,
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
pub(crate) struct TestLinuxTimespec {
    pub(crate) tv_sec: i64,
    pub(crate) tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxStack {
    pub(crate) ss_sp: u64,
    pub(crate) ss_flags: i32,
    pub(crate) ss_size: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxSigAction {
    pub(crate) handler: usize,
    pub(crate) flags: u64,
    pub(crate) restorer: usize,
    pub(crate) mask: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxSigSetArg {
    pub(crate) sigmask: u64,
    pub(crate) sigsetsize: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxItimerspec {
    pub(crate) it_interval: TestLinuxTimespec,
    pub(crate) it_value: TestLinuxTimespec,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxItimerval {
    pub(crate) it_interval: TestLinuxTimeval,
    pub(crate) it_value: TestLinuxTimeval,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct TestWaitidSigInfo {
    pub(crate) si_signo: i32,
    pub(crate) si_errno: i32,
    pub(crate) si_code: i32,
    pub(crate) _pad0: i32,
    pub(crate) si_pid: i32,
    pub(crate) si_uid: u32,
    pub(crate) si_status: i32,
    pub(crate) _pad1: i32,
    pub(crate) si_utime: i64,
    pub(crate) si_stime: i64,
    pub(crate) _rest: [u8; 80],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TestLinuxRlimit64 {
    rlim_cur: u64,
    rlim_max: u64,
}

pub(crate) fn close_test_fd(fd: usize) {
    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
        0,
    );
}

pub(crate) fn expect_fd(result: Result<usize, SyscallError>) -> usize {
    result.expect("syscall should create a file descriptor")
}

pub(crate) fn assert_fd_flags(fd: usize, expected: FdFlags) {
    let fd_table = get_current_process().lock().fd_table.clone();
    let fd_table = fd_table.lock();
    let flags = fd_table
        .get(fd)
        .and_then(|entry| entry.as_ref())
        .map(|entry| entry.fd_flags)
        .expect("test fd should exist");
    assert_eq!(flags, expected);
}

pub(crate) fn assert_object_flags(fd: usize, expected: FileFlags) {
    let flags = get_object_current_process(fd as u64)
        .expect("test fd should resolve")
        .get_flags()
        .expect("test object should report flags");
    assert_eq!(flags, expected);
}

pub(crate) fn assert_same_object(left_fd: usize, right_fd: usize) {
    let left = get_object_current_process(left_fd as u64).expect("left fd should resolve");
    let right = get_object_current_process(right_fd as u64).expect("right fd should resolve");
    assert!(alloc::sync::Arc::ptr_eq(&left, &right));
}

pub(crate) fn occupied_fd_count() -> usize {
    let fd_table = get_current_process().lock().fd_table.clone();
    fd_table.lock().iter().flatten().count()
}

pub(crate) fn write_user_cstr(addr: u64, value: &[u8]) {
    assert_eq!(value.last(), Some(&0));
    get_current_process()
        .lock()
        .addrspace
        .write_buffer(addr as *mut u8, value)
        .expect("test user c string should be writable");
}

pub(crate) fn allocate_large_user_test_region(pages: u64) -> u64 {
    let process = get_current_process();
    let mut process = process.lock();
    process.addrspace.allocate_user(pages).0.as_u64()
}

pub(crate) fn read_user_bytes(addr: u64, len: usize) -> Vec<u8> {
    get_current_process()
        .lock()
        .addrspace
        .read_buffer(addr as *const u8, len)
        .expect("test user address should be readable")
}

pub(crate) fn read_file_via_fd(fd: usize, page: u64, offset: u64, max_len: usize) -> Vec<u8> {
    expect_ok(
        SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
        0,
    );
    let read = SyscallArgs::new([fd as u64, page + offset, max_len as u64, 0, 0, 0])
        .call::<Read>()
        .expect("read should succeed");
    read_user_bytes(page + offset, read)
}

pub(crate) fn openat_fd(dirfd: u64, path_addr: u64, flags: OpenFlags) -> usize {
    expect_fd(SyscallArgs::new([dirfd, path_addr, flags.bits() as u64, 0, 0, 0]).call::<OpenAt>())
}

pub(crate) fn readlink_bytes(dirfd: u64, path_addr: u64, buf_addr: u64, buf_len: usize) -> Vec<u8> {
    let read = if dirfd == (-1i32) as u64 {
        SyscallArgs::new([path_addr, buf_addr, buf_len as u64, 0, 0, 0])
            .call::<Readlink>()
            .expect("readlink should succeed")
    } else {
        SyscallArgs::new([dirfd, path_addr, buf_addr, buf_len as u64, 0, 0])
            .call::<ReadlinkAt>()
            .expect("readlinkat should succeed")
    };
    read_user_bytes(buf_addr, read)
}

pub(crate) fn parse_dirent_names(addr: u64, bytes: usize) -> Vec<(String, u8)> {
    let mut offset = 0usize;
    let mut names = Vec::new();
    while offset < bytes {
        let entry = read_user_value::<LinuxDirent64Header>(addr + offset as u64);
        assert!(entry.d_reclen as usize >= 24);
        let name_len = entry.d_reclen as usize - 19;
        let raw_name = read_user_bytes(addr + offset as u64 + 19, name_len);
        let nul = raw_name
            .iter()
            .position(|byte| *byte == 0)
            .expect("dirent name should be nul terminated");
        let name = core::str::from_utf8(&raw_name[..nul])
            .expect("dirent name should be utf8")
            .to_string();
        names.push((name, entry.d_type));
        offset += entry.d_reclen as usize;
    }
    names
}

pub(crate) fn getdents_names(
    fd: usize,
    page: u64,
    offset: u64,
    capacity: usize,
) -> Vec<(String, u8)> {
    let bytes = SyscallArgs::new([fd as u64, page + offset, capacity as u64, 0, 0, 0])
        .call::<crate::systemcall::implementations::Getdents64>()
        .expect("getdents64 should succeed");
    parse_dirent_names(page + offset, bytes)
}

pub(crate) fn filesystem_path_state_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_create_link_syscalls_follow_linux_rules() {
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

pub(crate) fn opened_file_object_stat_keeps_mount_device_id_without_reborrowing_vfs() {
    let base_path = Path::new("/tmp/opened-file-object-stat-test");
    let file_path = Path::new("/tmp/opened-file-object-stat-test/file");
    let _ = VirtualFS.lock().delete_file(file_path.clone());
    let _ = VirtualFS.lock().delete_file(base_path.clone());
    VirtualFS.lock().create_dir(base_path.clone()).unwrap();
    VirtualFS.lock().create_file(file_path.clone()).unwrap();

    let opened = {
        let mut vfs = VirtualFS.lock();
        vfs.open(file_path.clone()).unwrap()
    };

    let stat = opened.stat();
    assert_eq!(stat.st_dev, mount_device_id_for_path(&file_path));
    assert_eq!(stat.st_mode & 0o170000, 0o100000);

    let _ = VirtualFS.lock().delete_file(file_path);
    let _ = VirtualFS.lock().delete_file(base_path);
}

pub(crate) fn filesystem_fd_state_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_metadata_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_io_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_rename_syscalls_follow_linux_rules() {
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

    VirtualFS
        .lock()
        .create_file(Path::new("/tmp/syscall-rename-test/src"))
        .unwrap();
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

pub(crate) fn filesystem_getdents_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_file_object_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_file_metadata_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_xattr_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_statx_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_name_to_handle_short_buffer_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_name_to_handle_success_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_name_to_handle_null_handle_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_name_to_handle_null_mount_id_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_name_to_handle_bad_flag_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_utimensat_success_syscalls_follow_linux_rules() {
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

pub(crate) fn prepare_utimensat_test_file() -> (usize, [u64; 2]) {
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

pub(crate) fn cleanup_utimensat_test_file(file_fd: usize) {
    let cleanup_paths = [
        "/tmp/syscall-utimensat-test/file",
        "/tmp/syscall-utimensat-test",
    ];
    close_test_fd(file_fd);
    for path in cleanup_paths {
        let _ = VirtualFS.lock().delete_file(Path::new(path));
    }
}

pub(crate) fn filesystem_utimensat_negative_nsec_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_utimensat_null_path_empty_path_syscalls_follow_linux_rules() {
    const AT_EMPTY_PATH: u64 = 0x1000;

    let (file_fd, [_user_page, times_page]) = prepare_utimensat_test_file();
    write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
    expect_errno(
        SyscallArgs::new([file_fd as u64, 0, times_page, AT_EMPTY_PATH, 0, 0]).call::<Utimensat>(),
        SyscallError::InvalidArguments,
    );

    cleanup_utimensat_test_file(file_fd);
}

pub(crate) fn filesystem_utimensat_empty_path_without_flag_syscalls_follow_linux_rules() {
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

pub(crate) fn filesystem_utimensat_at_fdcwd_null_path_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let (file_fd, [_user_page, times_page]) = prepare_utimensat_test_file();
    write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
    expect_errno(
        SyscallArgs::new([AT_FDCWD, 0, times_page, 0, 0, 0]).call::<Utimensat>(),
        SyscallError::BadAddress,
    );

    cleanup_utimensat_test_file(file_fd);
}

pub(crate) fn filesystem_utimensat_invalid_flag_syscalls_follow_linux_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;

    let (file_fd, [user_page, times_page]) = prepare_utimensat_test_file();
    write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
    expect_errno(
        SyscallArgs::new([AT_FDCWD, user_page, times_page, 0x200, 0, 0]).call::<Utimensat>(),
        SyscallError::InvalidArguments,
    );

    cleanup_utimensat_test_file(file_fd);
}

pub(crate) fn procfs_syscalls_follow_linux_proc_abi_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const O_WRONLY: u64 = 1;
    const O_DIRECTORY: u64 = 0o200000;
    const STATX_BASIC_STATS: u64 = 0x0000_07ff;
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;

    let page = allocate_large_user_test_region(4);
    let current_pid = get_current_process().lock().pid.0;
    let proc_pid_path = format!("/proc/{current_pid}/status\0");

    write_user_cstr(page, b"/proc/self/status\0");
    write_user_cstr(page + 128, proc_pid_path.as_bytes());
    write_user_cstr(page + 256, b"/proc/self\0");
    write_user_cstr(page + 384, b"/proc/self/root\0");
    write_user_cstr(page + 512, b"/proc/self/ns/net\0");
    write_user_cstr(page + 640, b"/proc/self/fd\0");
    write_user_cstr(page + 768, b"/proc/self/fdinfo\0");
    write_user_cstr(page + 896, b"/proc\0");
    write_user_cstr(page + 1024, b"/proc/pressure\0");
    write_user_cstr(page + 1152, b"/proc/sys/kernel/random\0");
    write_user_cstr(page + 1280, b"/proc/sys/kernel/hostname\0");
    write_user_cstr(page + 1408, b"/proc/sys/kernel/domainname\0");
    write_user_cstr(page + 1536, b"/proc/sys/fs/file-max\0");
    write_user_cstr(page + 1664, b"/proc/sys/fs/nr_open\0");
    write_user_cstr(page + 1792, b"/proc/self/oom_score_adj\0");
    write_user_cstr(page + 1920, b"/proc/self/uid_map\0");
    write_user_cstr(page + 2048, b"/proc/self/gid_map\0");
    write_user_cstr(page + 2176, b"/proc/self/setgroups\0");
    write_user_cstr(page + 2304, b"/proc/pressure/cpu\0");
    write_user_cstr(page + 2432, b"/proc/stat\0");
    write_user_cstr(page + 2560, b"/proc/uptime\0");
    write_user_cstr(page + 2688, b"/proc/mounts\0");
    write_user_cstr(page + 2816, b"/proc/self/mountinfo\0");

    let self_status_fd = openat_fd(AT_FDCWD, page, OpenFlags::empty());
    let pid_status_fd = openat_fd(AT_FDCWD, page + 128, OpenFlags::empty());
    let self_status = read_file_via_fd(self_status_fd, page, 2944, 512);
    let pid_status = read_file_via_fd(pid_status_fd, page, 3584, 512);
    let self_status = core::str::from_utf8(&self_status).unwrap();
    let pid_status = core::str::from_utf8(&pid_status).unwrap();
    assert!(self_status.contains(&format!("Pid:\t{current_pid}\n")));
    assert!(pid_status.contains(&format!("Pid:\t{current_pid}\n")));
    close_test_fd(self_status_fd);
    close_test_fd(pid_status_fd);

    let self_target = readlink_bytes((-1i32) as u64, page + 256, page + 3200, 64);
    assert_eq!(
        core::str::from_utf8(&self_target).unwrap(),
        format!("{current_pid}")
    );
    let root_target = readlink_bytes((-1i32) as u64, page + 384, page + 3264, 64);
    assert_eq!(core::str::from_utf8(&root_target).unwrap(), "/");

    expect_ok(
        SyscallArgs::new([
            AT_FDCWD,
            page + 512,
            AT_SYMLINK_NOFOLLOW,
            STATX_BASIC_STATS,
            page + 3328,
            0,
        ])
        .call::<Statx>(),
        0,
    );
    let net_ns_statx = read_user_value::<TestLinuxStatx>(page + 3328);
    assert_eq!(net_ns_statx.stx_mode & 0o170000, 0o100000);
    assert!(net_ns_statx.stx_ino != 0);

    let known_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
    let fd_path = format!("/proc/self/fd/{known_fd}\0");
    let fdinfo_path = format!("/proc/self/fdinfo/{known_fd}\0");
    write_user_cstr(page + 3840, fd_path.as_bytes());
    write_user_cstr(page + 3968, fdinfo_path.as_bytes());
    let fd_target = readlink_bytes((-1i32) as u64, page + 3840, page + 4096, 128);
    assert_eq!(
        core::str::from_utf8(&fd_target).unwrap(),
        "anon_inode:[kernel::object::linux_anon::EventFdObject]"
    );
    let fdinfo_fd = openat_fd(AT_FDCWD, page + 3968, OpenFlags::empty());
    let fdinfo = read_file_via_fd(fdinfo_fd, page, 4224, 256);
    let fdinfo = core::str::from_utf8(&fdinfo).unwrap();
    assert!(fdinfo.contains("pos:\t0\n"));
    assert!(fdinfo.contains("flags:\t0\n"));
    assert!(fdinfo.contains("mnt_id:\t0\n"));
    assert!(fdinfo.contains("ino:\t0\n"));
    close_test_fd(fdinfo_fd);
    close_test_fd(known_fd);

    for (path_addr, expected) in [
        (
            page + 896,
            vec!["self".to_string(), current_pid.to_string()],
        ),
        (
            page + 1024,
            vec!["cpu".to_string(), "io".to_string(), "memory".to_string()],
        ),
        (page + 1152, vec!["boot_id".to_string(), "uuid".to_string()]),
    ] {
        let dir_fd = openat_fd(AT_FDCWD, path_addr, OpenFlags::DIRECTORY);
        let names = getdents_names(dir_fd, page, 4480, 1024)
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        for item in expected {
            assert!(names.contains(&item), "missing {item} in getdents output");
        }
        close_test_fd(dir_fd);
    }

    let hostname_snapshot_fd = openat_fd(AT_FDCWD, page + 1280, OpenFlags::empty());
    let hostname_before =
        core::str::from_utf8(&read_file_via_fd(hostname_snapshot_fd, page, 5632, 128))
            .unwrap()
            .trim()
            .to_string();
    close_test_fd(hostname_snapshot_fd);
    let domain_snapshot_fd = openat_fd(AT_FDCWD, page + 1408, OpenFlags::empty());
    let domain_before =
        core::str::from_utf8(&read_file_via_fd(domain_snapshot_fd, page, 5760, 128))
            .unwrap()
            .trim()
            .to_string();
    close_test_fd(domain_snapshot_fd);

    let rw_cases = [
        (
            page + 1280,
            b"proc-syscall-host\n".as_slice(),
            "proc-syscall-host\n",
        ),
        (
            page + 1408,
            b"proc-syscall-domain\n".as_slice(),
            "proc-syscall-domain\n",
        ),
        (page + 1536, b"456789\n".as_slice(), "456789\n"),
        (page + 1664, b"654321\n".as_slice(), "654321\n"),
        (page + 1792, b"321\n".as_slice(), "321\n"),
        (page + 1920, b"0 1000 1".as_slice(), "0 1000 1\n"),
        (page + 2048, b"0 1000 1".as_slice(), "0 1000 1\n"),
        (page + 2176, b"deny".as_slice(), "deny\n"),
    ];
    for (index, (path_addr, payload, expected)) in rw_cases.into_iter().enumerate() {
        let payload_addr = page + 5888 + (index as u64 * 64);
        let read_addr = page + 6656 + (index as u64 * 64);
        get_current_process()
            .lock()
            .addrspace
            .write_buffer(payload_addr as *mut u8, payload)
            .expect("test payload should be writable");
        let fd = openat_fd(AT_FDCWD, path_addr, OpenFlags::empty());
        expect_ok(
            SyscallArgs::new([fd as u64, payload_addr, payload.len() as u64, 0, 0, 0])
                .call::<Write>(),
            payload.len(),
        );
        let rendered = read_file_via_fd(fd, page, read_addr - page, 128);
        assert_eq!(core::str::from_utf8(&rendered).unwrap(), expected);
        close_test_fd(fd);
    }

    let restore_hostname_fd = openat_fd(AT_FDCWD, page + 1280, OpenFlags::empty());
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((page + 7424) as *mut u8, hostname_before.as_bytes())
        .expect("hostname restore payload should be writable");
    expect_ok(
        SyscallArgs::new([
            restore_hostname_fd as u64,
            page + 7424,
            hostname_before.len() as u64,
            0,
            0,
            0,
        ])
        .call::<Write>(),
        hostname_before.len(),
    );
    close_test_fd(restore_hostname_fd);
    let restore_domain_fd = openat_fd(AT_FDCWD, page + 1408, OpenFlags::empty());
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((page + 7552) as *mut u8, domain_before.as_bytes())
        .expect("domain restore payload should be writable");
    expect_ok(
        SyscallArgs::new([
            restore_domain_fd as u64,
            page + 7552,
            domain_before.len() as u64,
            0,
            0,
            0,
        ])
        .call::<Write>(),
        domain_before.len(),
    );
    close_test_fd(restore_domain_fd);

    let invalid_numeric_fd =
        expect_fd(SyscallArgs::new([AT_FDCWD, page + 1536, O_WRONLY, 0, 0, 0]).call::<OpenAt>());
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((page + 7680) as *mut u8, b"not-a-number")
        .expect("invalid numeric payload should be writable");
    expect_errno(
        SyscallArgs::new([invalid_numeric_fd as u64, page + 7680, 12, 0, 0, 0]).call::<Write>(),
        SyscallError::IOError,
    );
    close_test_fd(invalid_numeric_fd);
    let invalid_oom_fd =
        expect_fd(SyscallArgs::new([AT_FDCWD, page + 1792, O_WRONLY, 0, 0, 0]).call::<OpenAt>());
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((page + 7808) as *mut u8, b"1001")
        .expect("invalid oom payload should be writable");
    expect_errno(
        SyscallArgs::new([invalid_oom_fd as u64, page + 7808, 4, 0, 0, 0]).call::<Write>(),
        SyscallError::IOError,
    );
    close_test_fd(invalid_oom_fd);

    let pressure_fd = openat_fd(AT_FDCWD, page + 2304, OpenFlags::empty());
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((page + 7936) as *mut u8, b"some 150000 1000000")
        .expect("pressure payload should be writable");
    expect_ok(
        SyscallArgs::new([pressure_fd as u64, page + 7936, 19, 0, 0, 0]).call::<Write>(),
        19,
    );
    let pressure = read_file_via_fd(pressure_fd, page, 8064, 128);
    let pressure = core::str::from_utf8(&pressure).unwrap();
    assert!(pressure.contains("some avg10=0.00"));
    assert!(pressure.contains("full avg10=0.00"));
    close_test_fd(pressure_fd);

    for (path_addr, expected_fragments) in [
        (page + 2432, vec!["cpu  ", "btime ", "processes "]),
        (page + 2560, vec![".", "\n"]),
        (page + 2688, vec![" proc ", " sysfs ", " devtmpfs "]),
        (page + 2816, vec![" /proc ", " /sys ", " /dev "]),
    ] {
        let fd = openat_fd(AT_FDCWD, path_addr, OpenFlags::empty());
        let rendered = read_file_via_fd(fd, page, 8192, 2048);
        let rendered = core::str::from_utf8(&rendered).unwrap();
        for fragment in expected_fragments {
            assert!(
                rendered.contains(fragment),
                "missing {fragment} in {rendered}"
            );
        }
        close_test_fd(fd);
    }

    expect_errno(
        SyscallArgs::new([AT_FDCWD, page + 2432, O_DIRECTORY, 0, 0, 0]).call::<OpenAt>(),
        SyscallError::NotADirectory,
    );
}

pub(crate) fn sysfs_syscalls_follow_linux_sysfs_abi_rules() {
    const AT_FDCWD: u64 = (-100i32) as u64;
    const O_WRONLY: u64 = 1;
    const O_DIRECTORY: u64 = 0o200000;
    const AF_NETLINK: u64 = 16;
    const SOCK_DGRAM: u64 = 2;
    const SOL_NETLINK: u64 = 270;
    const NETLINK_KOBJECT_UEVENT: u64 = 15;
    const NETLINK_ADD_MEMBERSHIP: u64 = 1;
    const STATX_BASIC_STATS: u64 = 0x0000_07ff;
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;

    let page = allocate_user_test_page();
    write_user_cstr(page, b"/sys/class/tty/tty0/active\0");
    write_user_cstr(page + 128, b"/sys/devices/platform/i8042/uevent\0");
    write_user_cstr(page + 256, b"/sys/class/graphics/fb0/device/subsystem\0");
    write_user_cstr(page + 384, b"/sys/class/input/event0/device/subsystem\0");
    write_user_cstr(page + 512, b"/sys/class\0");
    write_user_cstr(page + 640, b"/sys/devices/platform\0");
    write_user_cstr(page + 768, b"/sys/dev/char\0");
    write_user_cstr(page + 896, b"/sys/devices/platform/uevent\0");
    write_user_cstr(page + 1024, b"/sys/devices\0");

    let active_fd = openat_fd(AT_FDCWD, page, OpenFlags::empty());
    let active = read_file_via_fd(active_fd, page, 1152, 64);
    let active = core::str::from_utf8(&active).unwrap();
    assert!(active.starts_with("tty"));
    assert!(active.ends_with('\n'));
    close_test_fd(active_fd);

    let i8042_fd = openat_fd(AT_FDCWD, page + 128, OpenFlags::empty());
    let i8042 = read_file_via_fd(i8042_fd, page, 1216, 128);
    let i8042 = core::str::from_utf8(&i8042).unwrap();
    assert!(i8042.contains("DRIVER=i8042"));
    assert!(i8042.contains("SUBSYSTEM=platform"));
    close_test_fd(i8042_fd);

    let fb_subsystem = readlink_bytes((-1i32) as u64, page + 256, page + 1344, 128);
    assert_eq!(
        core::str::from_utf8(&fb_subsystem).unwrap(),
        "/sys/bus/platform"
    );
    let input_subsystem = readlink_bytes((-1i32) as u64, page + 384, page + 1472, 128);
    assert_eq!(
        core::str::from_utf8(&input_subsystem).unwrap(),
        "/sys/class/input"
    );
    expect_ok(
        SyscallArgs::new([
            AT_FDCWD,
            page + 384,
            AT_SYMLINK_NOFOLLOW,
            STATX_BASIC_STATS,
            page + 1600,
            0,
        ])
        .call::<Statx>(),
        0,
    );
    let link_statx = read_user_value::<TestLinuxStatx>(page + 1600);
    assert_eq!(link_statx.stx_mode & 0o170000, 0o120000);

    for (path_addr, expected) in [
        (
            page + 512,
            vec![
                "drm".to_string(),
                "graphics".to_string(),
                "input".to_string(),
                "tty".to_string(),
            ],
        ),
        (
            page + 640,
            vec![
                "uevent".to_string(),
                "i8042".to_string(),
                "seele-drm".to_string(),
            ],
        ),
        (
            page + 768,
            vec![
                "13:64".to_string(),
                "13:65".to_string(),
                "226:0".to_string(),
            ],
        ),
    ] {
        let fd = openat_fd(AT_FDCWD, path_addr, OpenFlags::DIRECTORY);
        let names = getdents_names(fd, page, 1856, 1024)
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        for item in expected {
            assert!(names.contains(&item), "missing {item} in sysfs getdents");
        }
        close_test_fd(fd);
    }

    let uevent_sock = expect_fd(
        SyscallArgs::new([AF_NETLINK, SOCK_DGRAM, NETLINK_KOBJECT_UEVENT, 0, 0, 0])
            .call::<Socket>(),
    );
    write_user_value(page + 1984, &1i32);
    expect_ok(
        SyscallArgs::new([
            uevent_sock as u64,
            SOL_NETLINK,
            NETLINK_ADD_MEMBERSHIP,
            page + 1984,
            4,
            0,
        ])
        .call::<Setsockopt>(),
        0,
    );
    let uevent_fd =
        expect_fd(SyscallArgs::new([AT_FDCWD, page + 896, O_WRONLY, 0, 0, 0]).call::<OpenAt>());
    let uevent_payload = b"add synthetic-uuid ACTION=spoof DEVPATH=/fake KEY=VALUE";
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((page + 2048) as *mut u8, uevent_payload)
        .expect("uevent payload should be writable");
    expect_ok(
        SyscallArgs::new([
            uevent_fd as u64,
            page + 2048,
            uevent_payload.len() as u64,
            0,
            0,
            0,
        ])
        .call::<Write>(),
        uevent_payload.len(),
    );
    write_user_value(page + 2816, &12u32);
    let recv_len = SyscallArgs::new([
        uevent_sock as u64,
        page + 2112,
        512,
        0,
        page + 2688,
        page + 2816,
    ])
    .call::<Recvfrom>()
    .expect("uevent recvfrom should succeed");
    let uevent_bytes = read_user_bytes(page + 2112, recv_len);
    let uevent_text = uevent_bytes
        .split(|byte| *byte == 0)
        .filter(|segment| !segment.is_empty())
        .map(|segment| core::str::from_utf8(segment).unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(uevent_text[0], "add@/devices/platform");
    assert!(uevent_text.iter().any(|line| line == "ACTION=add"));
    assert!(
        uevent_text
            .iter()
            .any(|line| line == "DEVPATH=/devices/platform")
    );
    assert!(uevent_text.iter().any(|line| line == "SUBSYSTEM=platform"));
    assert!(uevent_text.iter().any(|line| line == "SYNTH_ARG_KEY=VALUE"));
    assert!(
        uevent_text
            .iter()
            .any(|line| line == "SYNTH_ARG_ACTION=spoof")
    );
    assert!(
        uevent_text
            .iter()
            .any(|line| line == "SYNTH_ARG_DEVPATH=/fake")
    );
    assert!(
        uevent_text
            .iter()
            .any(|line| line == "SYNTH_UUID=synthetic-uuid")
    );
    let seq_line = uevent_text
        .iter()
        .find(|line| line.starts_with("SEQNUM="))
        .expect("uevent should include seqnum");
    assert!(seq_line[7..].parse::<u64>().is_ok());
    close_test_fd(uevent_fd);
    close_test_fd(uevent_sock);

    expect_errno(
        SyscallArgs::new([AT_FDCWD, page, O_DIRECTORY, 0, 0, 0]).call::<OpenAt>(),
        SyscallError::NotADirectory,
    );
    let readonly_active_fd = openat_fd(AT_FDCWD, page, OpenFlags::empty());
    get_current_process()
        .lock()
        .addrspace
        .write_buffer((page + 2432) as *mut u8, b"tty2")
        .expect("readonly payload should be writable");
    expect_errno(
        SyscallArgs::new([readonly_active_fd as u64, page + 2432, 4, 0, 0, 0]).call::<Write>(),
        SyscallError::ReadOnlyFileSystem,
    );
    close_test_fd(readonly_active_fd);
}

pub(crate) fn socket_name_and_shutdown_syscalls_follow_linux_rules() {
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
    const POLLRDHUP: i16 = 0x2000;
    const SO_TYPE: u64 = 3;
    const SO_ERROR: u64 = 4;
    const SO_SNDBUF: u64 = 7;
    const SO_PRIORITY: u64 = 12;
    const SO_PASSCRED: u64 = 16;
    const SO_PEERCRED: u64 = 17;
    const SO_ACCEPTCONN: u64 = 30;
    const SO_PROTOCOL: u64 = 38;
    const SO_DOMAIN: u64 = 39;
    const SO_PEERPIDFD: u64 = 77;
    const TCP_NODELAY: u64 = 1;

    assert_linux_layout::<TestLinuxSockAddrUn>(110, 2);
    assert_linux_layout::<TestLinuxSockAddrIn>(16, 2);

    let saved = CredentialSnapshot::save_current();
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
            events: POLLIN | POLLOUT | POLLHUP | POLLRDHUP,
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
    assert_eq!(peer_shutdown_poll.revents & POLLHUP, 0);
    assert_eq!(peer_shutdown_poll.revents & POLLRDHUP, POLLRDHUP);
    expect_ok(
        SyscallArgs::new([left_fd as u64, page + 58, 1, 0, 0, 0]).call::<Read>(),
        0,
    );
    write_user_value(page + 768, b"q");
    expect_errno(
        SyscallArgs::new([right_fd as u64, page + 768, 1, 0, 0, 0]).call::<Write>(),
        SyscallError::BrokenPipe,
    );
    write_user_value(page + 776, b"r");
    expect_ok(
        SyscallArgs::new([left_fd as u64, page + 776, 1, 0, 0, 0]).call::<Write>(),
        1,
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
    write_user_value(page + 1052, &6i32);
    expect_ok(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_PRIORITY,
            page + 1052,
            4,
            0,
        ])
        .call::<Setsockopt>(),
        0,
    );
    write_user_value(page + 1060, &4u32);
    expect_ok(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_PRIORITY,
            page + 1068,
            page + 1060,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1068), 6);
    {
        let process = get_current_process();
        let mut process = process.lock();
        process.effective_uid = 1000;
        process.capability_effective = [0; 2];
    }
    write_user_value(page + 1052, &7i32);
    expect_errno(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_PRIORITY,
            page + 1052,
            4,
            0,
        ])
        .call::<Setsockopt>(),
        SyscallError::PermissionDenied,
    );
    write_user_value(page + 1052, &(-1i32));
    expect_errno(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_PRIORITY,
            page + 1052,
            4,
            0,
        ])
        .call::<Setsockopt>(),
        SyscallError::PermissionDenied,
    );
    write_user_value(page + 1060, &4u32);
    expect_ok(
        SyscallArgs::new([
            inet_socket as u64,
            SOL_SOCKET,
            SO_PRIORITY,
            page + 1068,
            page + 1060,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1068), 6);
    saved.restore();
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
    write_user_value(page + 1152, &4u32);
    expect_ok(
        SyscallArgs::new([
            netlink_socket as u64,
            SOL_SOCKET,
            SO_PRIORITY,
            page + 1160,
            page + 1152,
            0,
        ])
        .call::<Getsockopt>(),
        0,
    );
    assert_eq!(read_user_value::<i32>(page + 1160), 0);

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

pub(crate) fn socket_bind_connect_accept_syscalls_follow_linux_rules() {
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

pub(crate) fn socket_message_syscalls_follow_linux_rules() {
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

    write_user_value(page + 1424, b"R");
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
