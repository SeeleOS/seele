use crate::memory::utils::Mut;
use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc, vec, vec::Vec};
use bitflags::bitflags;
use core::sync::atomic::{AtomicI32, Ordering};
use lazy_static::lazy_static;
use num_enum::TryFromPrimitive;
use x86_64::VirtAddr;

use crate::memory::{
    addrspace::mem_area::{Data, MemoryArea},
    protection::Protection,
    user_safe,
};
use crate::misc::error::AsSyscallError;
use crate::misc::time::Time as KernelTime;
use crate::misc::{others::protection_to_page_flags, reboot as reboot_state, utsname::UtsName};
use crate::net::namespace::NetNamespace;
use crate::object::linux_anon::{EventFdFlags, EventFdObject, InotifyObject, PidFdObject};
use crate::object::misc::get_object_current_process;
use crate::object::{FileFlags, Object, misc::ObjectRef};
use crate::process::{
    FdFlags, Process,
    manager::{MANAGER, get_current_process},
    misc::{ProcessID, get_process_with_pid},
};
use crate::signal::{
    Signal,
    action::{SignalAction, SignalHandlingType, Signals},
    misc::default_signal_action_vec,
};
use crate::systemcall::utils::{SyscallError, SyscallImpl};
use crate::terminal::pty::create_pty;
use crate::thread::misc::with_current_thread;
use crate::thread::yielding::{
    BlockType, WakeType, block_current_with_sig_check, cancel_block, finish_block_current,
    prepare_block_current,
};
use crate::{NAME, define_syscall};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct CloneFlags: u64 {
        const VM = 0x0000_0100;
        const FS = 0x0000_0200;
        const FILES = 0x0000_0400;
        const SIGHAND = 0x0000_0800;
        const PIDFD = 0x0000_1000;
        const VFORK = 0x0000_4000;
        const NEWPID = 0x2000_0000;
        const NEWNS = 0x0002_0000;
        const NEWCGROUP = 0x0200_0000;
        const NEWUTS = 0x0400_0000;
        const NEWIPC = 0x0800_0000;
        const NEWUSER = 0x1000_0000;
        const NEWNET = 0x4000_0000;
        const THREAD = 0x0001_0000;
        const SETTLS = 0x0008_0000;
        const PARENT_SETTID = 0x0010_0000;
        const CHILD_CLEARTID = 0x0020_0000;
        const CHILD_SETTID = 0x0100_0000;
        const CLEAR_SIGHAND = 0x1_0000_0000;
        const INTO_CGROUP = 0x2_0000_0000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct UnshareFlags: u64 {
        const FS = CloneFlags::FS.bits();
        const FILES = CloneFlags::FILES.bits();
        const NEWNS = CloneFlags::NEWNS.bits();
        const SYSVSEM = 0x0004_0000;
        const NEWCGROUP = CloneFlags::NEWCGROUP.bits();
        const NEWUTS = CloneFlags::NEWUTS.bits();
        const NEWIPC = CloneFlags::NEWIPC.bits();
        const NEWUSER = CloneFlags::NEWUSER.bits();
        const NEWPID = CloneFlags::NEWPID.bits();
        const NEWNET = CloneFlags::NEWNET.bits();
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SetnsFlags: u32 {
        const NEWNET = CloneFlags::NEWNET.bits() as u32;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxCloneArgs {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(i32)]
enum PrctlOption {
    SetPdeathsig = 1,
    GetPdeathsig = 2,
    GetDumpable = 3,
    SetDumpable = 4,
    GetKeepCaps = 7,
    SetKeepCaps = 8,
    SetName = 15,
    GetName = 16,
    GetSeccomp = 21,
    SetSeccomp = 22,
    CapbsetRead = 23,
    CapbsetDrop = 24,
    GetSecureBits = 27,
    SetSecureBits = 28,
    SetChildSubreaper = 36,
    GetChildSubreaper = 37,
    SetNoNewPrivs = 38,
    GetNoNewPrivs = 39,
    CapAmbient = 47,
    SetMdwe = 65,
    GetMdwe = 66,
}

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum PrctlCapAmbientOp {
    IsSet = 1,
    Raise = 2,
    Lower = 3,
    ClearAll = 4,
}

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum KeyctlCommand {
    GetKeyringId = 0,
    JoinSessionKeyring = 1,
    Revoke = 3,
    Setperm = 5,
    Link = 8,
    SessionToParent = 18,
}

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u32)]
enum KcmpType {
    File = 0,
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct RseqFlags: u32 {
        const UNREGISTER = 1;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct InotifyInitFlags: i32 {
        const IN_NONBLOCK = 0o4_000;
        const IN_CLOEXEC = 0o2_000_000;
    }
}

const RSEQ_LEN_X86_64: u32 = 32;
const RSEQ_CPU_ID_UNINITIALIZED: u32 = u32::MAX;
const RSEQ_CPU_ID_SINGLE_CORE: u32 = 0;
const INITIAL_BRK_RESERVE: u64 = 0x4000_0000;
const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
const LINUX_CAPABILITY_U32S_3: usize = 2;
const LINUX_REBOOT_MAGIC1: u32 = 0xfee1_dead;
const LINUX_REBOOT_MAGIC2: u32 = 0x2812_1969;
const LINUX_REBOOT_CMD_CAD_OFF: u32 = 0x0000_0000;
const LINUX_REBOOT_CMD_CAD_ON: u32 = 0x89ab_cdef;
const KEY_SPEC_SESSION_KEYRING: i32 = -3;
const KEY_SPEC_USER_KEYRING: i32 = -4;
const KEY_SPEC_USER_SESSION_KEYRING: i32 = -5;

static NEXT_SESSION_KEYRING_ID: AtomicI32 = AtomicI32::new(1);
static NEXT_KEY_SERIAL: AtomicI32 = AtomicI32::new(1024);

lazy_static! {
    static ref KEY_REGISTRY: Mut<BTreeMap<i32, KeyEntry>> = Mut::new(BTreeMap::new());
}

#[derive(Clone, Debug, Default)]
struct KeyEntry {
    permissions: u32,
    links: Vec<i32>,
    is_keyring: bool,
    revoked: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxCapHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxCapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

fn capability_header_targets_current_process(header: &LinuxCapHeader) -> bool {
    header.pid == 0 || header.pid == get_current_process().lock().pid.0 as i32
}

fn current_capability_data() -> [LinuxCapData; LINUX_CAPABILITY_U32S_3] {
    let process = get_current_process();
    let process = process.lock();
    core::array::from_fn(|index| LinuxCapData {
        effective: process.capability_effective[index],
        permitted: process.capability_permitted[index],
        inheritable: process.capability_inheritable[index],
    })
}

fn capability_slot_and_mask(capability: u64) -> Result<(usize, u32), SyscallError> {
    let slot = (capability / 32) as usize;
    if slot >= LINUX_CAPABILITY_U32S_3 {
        return Err(SyscallError::InvalidArguments);
    }
    let mask = 1u32
        .checked_shl((capability % 32) as u32)
        .ok_or(SyscallError::InvalidArguments)?;
    Ok((slot, mask))
}

fn current_session_keyring(create: bool) -> Result<i32, SyscallError> {
    let process = get_current_process();
    let mut process = process.lock();
    if process.session_keyring == 0 {
        if !create {
            return Err(SyscallError::NoData);
        }
        process.session_keyring = NEXT_SESSION_KEYRING_ID.fetch_add(1, Ordering::Relaxed);
        ensure_keyring_entry(process.session_keyring);
    }
    Ok(process.session_keyring)
}

fn current_user_keyring(create: bool) -> Result<i32, SyscallError> {
    let process = get_current_process();
    let mut process = process.lock();
    if process.user_keyring == 0 {
        if !create {
            return Err(SyscallError::NoData);
        }
        process.user_keyring = NEXT_SESSION_KEYRING_ID.fetch_add(1, Ordering::Relaxed);
        ensure_keyring_entry(process.user_keyring);
    }
    Ok(process.user_keyring)
}

fn resolve_keyring(spec: i32, create: bool) -> Result<i32, SyscallError> {
    match spec {
        KEY_SPEC_SESSION_KEYRING => current_session_keyring(create),
        KEY_SPEC_USER_KEYRING => current_user_keyring(create),
        KEY_SPEC_USER_SESSION_KEYRING => current_session_keyring(create),
        serial if serial > 0 => {
            if create {
                ensure_keyring_entry(serial);
                Ok(serial)
            } else if keyring_exists(serial) {
                Ok(serial)
            } else {
                Err(SyscallError::InvalidArguments)
            }
        }
        _ => Err(SyscallError::InvalidArguments),
    }
}

fn keyring_exists(serial: i32) -> bool {
    KEY_REGISTRY
        .lock()
        .get(&serial)
        .is_some_and(|entry| entry.is_keyring && !entry.revoked)
}

fn ensure_keyring_entry(serial: i32) {
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry.entry(serial).or_default();
    entry.is_keyring = true;
}

fn ensure_key_entry(serial: i32) {
    KEY_REGISTRY.lock().entry(serial).or_default();
}

fn set_key_permissions(serial: i32, permissions: u32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry
        .get_mut(&serial)
        .ok_or(SyscallError::InvalidArguments)?;
    if entry.revoked {
        return Err(SyscallError::InvalidArguments);
    }
    entry.permissions = permissions;
    Ok(())
}

fn link_key_into_keyring(source: i32, target: i32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let Some(source_entry) = registry.get(&source) else {
        return Err(SyscallError::InvalidArguments);
    };
    if source_entry.revoked {
        return Err(SyscallError::InvalidArguments);
    }
    let target_entry = registry
        .get_mut(&target)
        .ok_or(SyscallError::InvalidArguments)?;
    if !target_entry.is_keyring || target_entry.revoked {
        return Err(SyscallError::InvalidArguments);
    }
    if !target_entry.links.contains(&source) {
        target_entry.links.push(source);
    }
    Ok(())
}

fn revoke_key(serial: i32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry
        .get_mut(&serial)
        .ok_or(SyscallError::InvalidArguments)?;
    entry.revoked = true;
    entry.links.clear();
    Ok(())
}

fn clone_cleared_signal_actions(old_actions: &[SignalAction]) -> Vec<SignalAction> {
    let defaults = default_signal_action_vec();
    old_actions
        .iter()
        .zip(defaults)
        .map(|(old, default)| match old.handling_type {
            SignalHandlingType::Ignore => old.clone(),
            SignalHandlingType::Default
            | SignalHandlingType::Function1(_)
            | SignalHandlingType::Function2(_) => default,
        })
        .collect()
}

fn process_fd_object(process: &Process, fd: usize) -> Result<ObjectRef, SyscallError> {
    process
        .fd_table
        .lock()
        .get(fd)
        .and_then(|entry| entry.as_ref())
        .map(|entry| entry.object.clone())
        .ok_or(SyscallError::BadFileDescriptor)
}

struct CloneProcessArgs {
    clone_flags: CloneFlags,
    raw_flags: u64,
    exit_signal: u8,
    stack_pointer: u64,
    parent_tid: *mut i32,
    child_tid: *mut i32,
    tls: u64,
    pidfd_ptr: *mut i32,
    cgroup_fd: u64,
}

fn wait_for_vfork_completion(child_process: &crate::process::ProcessRef) {
    loop {
        if child_process.lock().vfork_blocker.is_none() {
            return;
        }

        let current = prepare_block_current(BlockType::WakeRequired {
            wake_type: WakeType::ProcsesExit,
            deadline: None,
        });

        if child_process.lock().vfork_blocker.is_none() {
            cancel_block(&current);
            return;
        }

        finish_block_current();
    }
}

fn clone_process(args: CloneProcessArgs) -> Result<usize, SyscallError> {
    let CloneProcessArgs {
        clone_flags,
        raw_flags,
        exit_signal,
        stack_pointer,
        parent_tid,
        child_tid,
        tls,
        pidfd_ptr,
        cgroup_fd,
    } = args;
    let unsupported = raw_flags
        & !(0xff
            | CloneFlags::VM.bits()
            | CloneFlags::VFORK.bits()
            | CloneFlags::FS.bits()
            | CloneFlags::FILES.bits()
            | CloneFlags::NEWNS.bits()
            | CloneFlags::NEWCGROUP.bits()
            | CloneFlags::NEWUTS.bits()
            | CloneFlags::NEWIPC.bits()
            | CloneFlags::NEWUSER.bits()
            | CloneFlags::NEWPID.bits()
            | CloneFlags::NEWNET.bits()
            | CloneFlags::CLEAR_SIGHAND.bits()
            | CloneFlags::PARENT_SETTID.bits()
            | CloneFlags::CHILD_SETTID.bits()
            | CloneFlags::CHILD_CLEARTID.bits()
            | CloneFlags::SETTLS.bits()
            | CloneFlags::PIDFD.bits()
            | CloneFlags::INTO_CGROUP.bits());
    if unsupported != 0 || (exit_signal != 0 && exit_signal != 17) {
        return Err(SyscallError::NoSyscall);
    }
    if clone_flags.contains(CloneFlags::PIDFD) && pidfd_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }
    if clone_flags.contains(CloneFlags::PIDFD)
        && clone_flags.contains(CloneFlags::PARENT_SETTID)
        && core::ptr::eq(pidfd_ptr, parent_tid)
    {
        return Err(SyscallError::NoSyscall);
    }
    if clone_flags.contains(CloneFlags::INTO_CGROUP) && cgroup_fd == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let current = get_current_process();
    let is_vfork = clone_flags.contains(CloneFlags::VFORK);
    let share_fd_table = clone_flags.contains(CloneFlags::FILES);
    let share_fs_context = clone_flags.contains(CloneFlags::FS);
    let (child_process, child_thread) = if is_vfork {
        Process::vfork_with_sharing(current.clone(), share_fd_table, share_fs_context)
    } else {
        Process::fork_with_sharing(current.clone(), share_fd_table, share_fs_context)
    };
    if clone_flags.contains(CloneFlags::NEWNET) {
        child_process.lock().net_namespace = NetNamespace::new();
    }
    let pid = child_process.lock().pid;
    MANAGER.lock().processes.insert(pid, child_process.clone());

    if clone_flags.contains(CloneFlags::VFORK) {
        child_process.lock().vfork_blocker = Some(crate::thread::get_current_thread().lock().id);
    }

    if clone_flags.contains(CloneFlags::INTO_CGROUP) {
        let cgroup_path = get_object_current_process(cgroup_fd)
            .map_err(SyscallError::from)?
            .as_file_like()?
            .path();
        crate::filesystem::cgroupfs::set_pid_cgroup_path_from_fs_path(pid, &cgroup_path)
            .map_err(SyscallError::from)?;
    }

    if clone_flags.contains(CloneFlags::CLEAR_SIGHAND) {
        let mut child = child_process.lock();
        child.signal_actions = clone_cleared_signal_actions(&child.signal_actions);
    }

    {
        let mut child = child_thread.lock();
        if stack_pointer != 0 {
            child.snapshot.inner.rsp = stack_pointer;
        }
        child.snapshot.inner.rax = 0;
        if clone_flags.contains(CloneFlags::SETTLS) {
            child.snapshot.fs_base = tls;
        }
    }

    if clone_flags.contains(CloneFlags::PARENT_SETTID) {
        user_safe::write(parent_tid, &(pid.0 as i32))?;
    }

    if clone_flags.contains(CloneFlags::CHILD_SETTID) {
        child_process
            .lock()
            .addrspace
            .write(child_tid, &(pid.0 as i32))?;
    }

    if clone_flags.contains(CloneFlags::CHILD_CLEARTID) {
        child_thread.lock().clear_child_tid = child_tid as u64;
    }

    if clone_flags.contains(CloneFlags::PIDFD) {
        let pidfd: Arc<dyn Object> = PidFdObject::new(pid.0);
        let pidfd_fd = i32::try_from(
            current
                .lock()
                .push_object_with_flags(pidfd, FdFlags::CLOEXEC),
        )
        .map_err(|_| SyscallError::TooManyOpenFilesProcess)?;
        user_safe::write(pidfd_ptr, &pidfd_fd)?;
    }

    if is_vfork {
        // Keep vfork safe for multi-threaded parents by using the existing
        // fork/COW address-space clone and only adding the parent wait semantics.
        Process::wake_vfork_child(child_thread);
        wait_for_vfork_completion(&child_process);
    }

    Ok(pid.0 as usize)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
enum RlimitResource {
    Core = 4,
    Stack = 3,
    NoFile = 7,
    MemLock = 8,
    RtPrio = 14,
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct GetRandomFlags: u32 {
        const NONBLOCK = 0x0001;
        const RANDOM = 0x0002;
        const INSECURE = 0x0004;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxRlimit64 {
    rlim_cur: u64,
    rlim_max: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxTimeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxRusage {
    ru_utime: LinuxTimeval,
    ru_stime: LinuxTimeval,
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
struct LinuxSchedParam {
    sched_priority: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(i32)]
pub enum LinuxIoprioWho {
    Process = 1,
    Pgrp = 2,
    User = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(i32)]
enum LinuxRusageWho {
    Self_ = 0,
    Children = -1,
    Thread = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(u16)]
enum LinuxIoprioClass {
    None = 0,
    Realtime = 1,
    BestEffort = 2,
    Idle = 3,
}

const LINUX_IOPRIO_CLASS_SHIFT: u16 = 13;
const LINUX_IOPRIO_PRIO_MASK: u16 = (1 << LINUX_IOPRIO_CLASS_SHIFT) - 1;
const LINUX_IOPRIO_LEVEL_MAX: u16 = 7;

fn decode_linux_ioprio(ioprio: i32) -> Result<(LinuxIoprioClass, u16), SyscallError> {
    let raw = u16::try_from(ioprio).map_err(|_| SyscallError::InvalidArguments)?;
    let class = LinuxIoprioClass::try_from(raw >> LINUX_IOPRIO_CLASS_SHIFT)
        .map_err(|_| SyscallError::InvalidArguments)?;
    let level = raw & LINUX_IOPRIO_PRIO_MASK;
    if level > LINUX_IOPRIO_LEVEL_MAX {
        return Err(SyscallError::InvalidArguments);
    }
    Ok((class, level))
}

fn validate_linux_ioprio_target(which: LinuxIoprioWho, who: i32) -> Result<(), SyscallError> {
    if who < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    match which {
        LinuxIoprioWho::Process => {
            if who == 0 {
                return Ok(());
            }
            let current = get_current_process().lock().pid.0 as i32;
            if who != current {
                return Err(SyscallError::PermissionDenied);
            }
            Ok(())
        }
        LinuxIoprioWho::Pgrp | LinuxIoprioWho::User => {
            if who == 0 {
                Ok(())
            } else {
                Err(SyscallError::PermissionDenied)
            }
        }
    }
}

fn default_linux_ioprio() -> usize {
    ((LinuxIoprioClass::BestEffort as u16) << LINUX_IOPRIO_CLASS_SHIFT) as usize
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(i32)]
pub enum LinuxSchedPolicy {
    Other = 0,
    Fifo = 1,
    RoundRobin = 2,
    Batch = 3,
    Idle = 5,
    Deadline = 6,
}

impl LinuxSchedPolicy {
    fn min_priority(self) -> i32 {
        match self {
            Self::Fifo | Self::RoundRobin => 1,
            Self::Other | Self::Batch | Self::Idle | Self::Deadline => 0,
        }
    }

    fn max_priority(self) -> i32 {
        match self {
            Self::Fifo | Self::RoundRobin => 99,
            Self::Other | Self::Batch | Self::Idle | Self::Deadline => 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxRseq {
    cpu_id_start: u32,
    cpu_id: u32,
    rseq_cs: u64,
    flags: u32,
    _padding: u32,
    _padding2: u64,
}

fn write_rseq_area(rseq_ptr: *mut LinuxRseq, registered: bool) -> Result<(), SyscallError> {
    if rseq_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut rseq = LinuxRseq {
        cpu_id_start: RSEQ_CPU_ID_UNINITIALIZED,
        cpu_id: RSEQ_CPU_ID_UNINITIALIZED,
        rseq_cs: 0,
        flags: 0,
        _padding: 0,
        _padding2: 0,
    };
    if registered {
        rseq.cpu_id_start = RSEQ_CPU_ID_SINGLE_CORE;
        rseq.cpu_id = RSEQ_CPU_ID_SINGLE_CORE;
    }
    user_safe::write(rseq_ptr, &rseq)?;
    Ok(())
}

mod anon_fd;
mod capability;
mod identity;
mod keyring;
mod prctl;
mod process;
mod pty;
mod resource;
mod rseq;
mod scheduler;
mod system;

pub use anon_fd::*;
pub use capability::*;
pub use identity::*;
pub use keyring::*;
pub use prctl::*;
pub use process::*;
pub use pty::*;
pub use resource::*;
pub use rseq::*;
pub use scheduler::*;
pub use system::*;
