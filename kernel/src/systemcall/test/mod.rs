#![allow(unused_imports)]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::systemcall::implementations::{
    Close, Getdents64, Lseek, OpenAt, OpenFlags, Read, Readlink, ReadlinkAt,
};

pub(crate) use crate::systemcall::implementations::*;
pub(crate) use crate::{
    filesystem::{
        absolute_path::AbsolutePath, info::LinuxStat, object::mount_device_id_for_path, path::Path,
        vfs::VirtualFS,
    },
    memory::{addrspace::mem_area::Data, protection::Protection},
    misc::timer::ClockId,
    object::{FileFlags, misc::get_object_current_process, traits::Statable},
    polling::{event::PollableEvent, object::Pollable},
    process::{
        ControllingTerminal, FdFlags, Process, ProcessExitStatus,
        group::{ProcessGroupID, SessionID},
        manager::{MANAGER, get_current_process, terminate_process},
        misc::ProcessID,
    },
    signal::{SigInfo, Signal, Signals},
    systemcall::{
        arg_types::SyscallArg,
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
pub(crate) use alloc::{format, vec};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct LinuxDirent64Header {
    pub(crate) d_ino: u64,
    pub(crate) d_off: i64,
    pub(crate) d_reclen: u16,
    pub(crate) d_type: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxStatxTimestamp {
    pub(crate) tv_sec: i64,
    pub(crate) tv_nsec: u32,
    pub(crate) __reserved: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxStatx {
    pub(crate) stx_mask: u32,
    pub(crate) stx_blksize: u32,
    pub(crate) stx_attributes: u64,
    pub(crate) stx_nlink: u32,
    pub(crate) stx_uid: u32,
    pub(crate) stx_gid: u32,
    pub(crate) stx_mode: u16,
    pub(crate) __spare0: u16,
    pub(crate) stx_ino: u64,
    pub(crate) stx_size: u64,
    pub(crate) stx_blocks: u64,
    pub(crate) stx_attributes_mask: u64,
    pub(crate) stx_atime: TestLinuxStatxTimestamp,
    pub(crate) stx_btime: TestLinuxStatxTimestamp,
    pub(crate) stx_ctime: TestLinuxStatxTimestamp,
    pub(crate) stx_mtime: TestLinuxStatxTimestamp,
    pub(crate) stx_rdev_major: u32,
    pub(crate) stx_rdev_minor: u32,
    pub(crate) stx_dev_major: u32,
    pub(crate) stx_dev_minor: u32,
    pub(crate) stx_mnt_id: u64,
    pub(crate) stx_dio_mem_align: u32,
    pub(crate) stx_dio_offset_align: u32,
    pub(crate) __spare3: [u64; 12],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxFileHandle {
    pub(crate) handle_bytes: u32,
    pub(crate) handle_type: i32,
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
    pub(crate) sun_family: u16,
    pub(crate) sun_path: [u8; 108],
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
    pub(crate) sin_family: u16,
    pub(crate) sin_port: u16,
    pub(crate) sin_addr: [u8; 4],
    pub(crate) sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct TestLinuxSockAddrIn6 {
    pub(crate) sin6_family: u16,
    pub(crate) sin6_port: u16,
    pub(crate) sin6_flowinfo: u32,
    pub(crate) sin6_addr: [u8; 16],
    pub(crate) sin6_scope_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestRelibcIovec {
    pub(crate) iov_base: *mut u8,
    pub(crate) iov_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestRelibcMsgHdr {
    pub(crate) msg_name: *mut u8,
    pub(crate) msg_namelen: u32,
    pub(crate) msg_iov: *mut TestRelibcIovec,
    pub(crate) msg_iovlen: usize,
    pub(crate) msg_control: *mut u8,
    pub(crate) msg_controllen: usize,
    pub(crate) msg_flags: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestRelibcMmsghdr {
    pub(crate) msg_hdr: TestRelibcMsgHdr,
    pub(crate) msg_len: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxCmsgHdr {
    pub(crate) cmsg_len: usize,
    pub(crate) cmsg_level: i32,
    pub(crate) cmsg_type: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxUcred {
    pub(crate) pid: i32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestRightsControlMessage {
    pub(crate) header: TestLinuxCmsgHdr,
    pub(crate) fd: i32,
    pub(crate) pad: i32,
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

pub(crate) fn syscall_number_lookup_matches_x86_64_abi_values() {
    assert_eq!(SyscallNumber::from_number(0), Some(SyscallNumber::Read));
    assert_eq!(SyscallNumber::from_number(1), Some(SyscallNumber::Write));
    assert_eq!(SyscallNumber::from_number(4), Some(SyscallNumber::Stat));
    assert_eq!(SyscallNumber::from_number(6), Some(SyscallNumber::Lstat));
    assert_eq!(SyscallNumber::from_number(257), Some(SyscallNumber::OpenAt));
    assert_eq!(SyscallNumber::from_number(999), None);
}

fn syscall_table_contains_registered_and_rejects_unknown_numbers() {
    assert!(SYSCALL_TABLE[SyscallNumber::Read as usize].is_some());
    assert!(SYSCALL_TABLE[SyscallNumber::Stat as usize].is_some());
    assert!(SYSCALL_TABLE[SyscallNumber::Lstat as usize].is_some());
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

pub(crate) struct CredentialSnapshot {
    pub(crate) real_uid: u32,
    pub(crate) effective_uid: u32,
    pub(crate) saved_uid: u32,
    pub(crate) fs_uid: u32,
    pub(crate) real_gid: u32,
    pub(crate) effective_gid: u32,
    pub(crate) saved_gid: u32,
    pub(crate) fs_gid: u32,
    pub(crate) capability_effective: [u32; 2],
    pub(crate) capability_permitted: [u32; 2],
    pub(crate) capability_inheritable: [u32; 2],
    pub(crate) user_namespace_uid_map: Option<String>,
    pub(crate) user_namespace_gid_map: Option<String>,
    pub(crate) user_namespace_setgroups: Option<String>,
}

impl CredentialSnapshot {
    pub(crate) fn save(process: &Process) -> Self {
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
            user_namespace_uid_map: process.user_namespace_uid_map.clone(),
            user_namespace_gid_map: process.user_namespace_gid_map.clone(),
            user_namespace_setgroups: process.user_namespace_setgroups.clone(),
        }
    }

    pub(crate) fn save_current() -> Self {
        let process = get_current_process();
        let process = process.lock();
        Self::save(&process)
    }

    pub(crate) fn restore(self) {
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
        process.user_namespace_uid_map = self.user_namespace_uid_map;
        process.user_namespace_gid_map = self.user_namespace_gid_map;
        process.user_namespace_setgroups = self.user_namespace_setgroups;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxCapHeader {
    pub(crate) version: u32,
    pub(crate) pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxCapData {
    pub(crate) effective: u32,
    pub(crate) permitted: u32,
    pub(crate) inheritable: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxTimeval {
    pub(crate) tv_sec: i64,
    pub(crate) tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxTimezone {
    pub(crate) tz_minuteswest: i32,
    pub(crate) tz_dsttime: i32,
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
pub(crate) struct TestLinuxSysinfo {
    pub(crate) uptime: i64,
    pub(crate) loads: [u64; 3],
    pub(crate) totalram: u64,
    pub(crate) freeram: u64,
    pub(crate) sharedram: u64,
    pub(crate) bufferram: u64,
    pub(crate) totalswap: u64,
    pub(crate) freeswap: u64,
    pub(crate) procs: u16,
    pub(crate) _pad: u16,
    pub(crate) totalhigh: u64,
    pub(crate) freehigh: u64,
    pub(crate) mem_unit: u32,
    pub(crate) _f: [i8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxRseq {
    pub(crate) cpu_id_start: u32,
    pub(crate) cpu_id: u32,
    pub(crate) rseq_cs: u64,
    pub(crate) flags: u32,
    pub(crate) _padding: u32,
    pub(crate) _padding2: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct TestUtsName {
    pub(crate) sysname: [u8; 65],
    pub(crate) nodename: [u8; 65],
    pub(crate) release: [u8; 65],
    pub(crate) version: [u8; 65],
    pub(crate) machine: [u8; 65],
    pub(crate) domainname: [u8; 65],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TestLinuxCloneArgs {
    pub(crate) flags: u64,
    pub(crate) pidfd: u64,
    pub(crate) child_tid: u64,
    pub(crate) parent_tid: u64,
    pub(crate) exit_signal: u64,
    pub(crate) stack: u64,
    pub(crate) stack_size: u64,
    pub(crate) tls: u64,
    pub(crate) set_tid: u64,
    pub(crate) set_tid_size: u64,
    pub(crate) cgroup: u64,
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
pub(crate) struct TestLinuxRlimit64 {
    pub(crate) rlim_cur: u64,
    pub(crate) rlim_max: u64,
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
