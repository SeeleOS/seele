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
