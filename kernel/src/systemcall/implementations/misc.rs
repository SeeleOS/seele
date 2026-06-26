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
use crate::object::namespace::{NamespaceKind, NamespaceObject};
use crate::object::{FileFlags, Object, misc::ObjectRef};
use crate::process::{
    FdFlags, LinuxSchedPolicy, Process, ProcessRef,
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
use crate::thread::scheduling::return_to_scheduler_from_current;
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
        const PARENT = 0x0000_8000;
        const NEWPID = 0x2000_0000;
        const NEWNS = 0x0002_0000;
        const SYSVSEM = 0x0004_0000;
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
        const NEWIPC = CloneFlags::NEWIPC.bits() as u32;
        const NEWNS = CloneFlags::NEWNS.bits() as u32;
        const NEWPID = CloneFlags::NEWPID.bits() as u32;
        const NEWUTS = CloneFlags::NEWUTS.bits() as u32;
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
    Update = 2,
    Revoke = 3,
    Chown = 4,
    Setperm = 5,
    Describe = 6,
    Clear = 7,
    Link = 8,
    Unlink = 9,
    Search = 10,
    Read = 11,
    SetReqkeyKeyring = 14,
    SetTimeout = 15,
    SessionToParent = 18,
    Invalidate = 21,
    GetPersistent = 22,
    RestrictKeyring = 29,
    Move = 30,
    Capabilities = 31,
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
const LINUX_CAPABILITY_VERSION_1: u32 = 0x1998_0330;
const LINUX_CAPABILITY_VERSION_2: u32 = 0x2007_1026;
const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
const LINUX_CAPABILITY_U32S_1: usize = 1;
const LINUX_CAPABILITY_U32S_3: usize = 2;
const CAP_SETPCAP: usize = 8;
const LINUX_REBOOT_MAGIC1: u32 = 0xfee1_dead;
const LINUX_REBOOT_MAGIC2: u32 = 0x2812_1969;
const LINUX_REBOOT_CMD_CAD_OFF: u32 = 0x0000_0000;
const LINUX_REBOOT_CMD_CAD_ON: u32 = 0x89ab_cdef;
const KEY_SPEC_THREAD_KEYRING: i32 = -1;
const KEY_SPEC_PROCESS_KEYRING: i32 = -2;
const KEY_SPEC_SESSION_KEYRING: i32 = -3;
const KEY_SPEC_USER_KEYRING: i32 = -4;
const KEY_SPEC_USER_SESSION_KEYRING: i32 = -5;
const KEY_REQKEY_DEFL_DEFAULT: i32 = 0;
const KEY_REQKEY_DEFL_THREAD_KEYRING: i32 = 1;
const KEY_REQKEY_DEFL_PROCESS_KEYRING: i32 = 2;
const KEY_REQKEY_DEFL_SESSION_KEYRING: i32 = 3;
const KEY_POS_WRITE: u32 = 0x0400_0000;
const KEY_USER_MAX_PAYLOAD: usize = 32_767;
pub(crate) const KEY_USER_DEFAULT_MAX_KEYS: usize = 200;
pub(crate) const KEY_USER_DEFAULT_MAX_BYTES: usize = 200_000;
pub(crate) const KEY_ROOT_DEFAULT_MAX_KEYS: usize = 1_000_000;
pub(crate) const KEY_ROOT_DEFAULT_MAX_BYTES: usize = 25_000_000;

static NEXT_SESSION_KEYRING_ID: AtomicI32 = AtomicI32::new(1);
static NEXT_KEY_SERIAL: AtomicI32 = AtomicI32::new(1024);

lazy_static! {
    static ref KEY_REGISTRY: Mut<BTreeMap<i32, KeyEntry>> = Mut::new(BTreeMap::new());
    static ref USER_KEYRINGS: Mut<BTreeMap<(u32, UserKeyringKind), i32>> =
        Mut::new(BTreeMap::new());
    static ref KEY_USER_QUOTAS: Mut<BTreeMap<u32, KeyUserQuota>> = Mut::new(BTreeMap::new());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum UserKeyringKind {
    User,
    UserSession,
}

#[derive(Clone, Debug, Default)]
struct KeyEntry {
    type_name: String,
    description: String,
    payload: Vec<u8>,
    uid: u32,
    gid: u32,
    permissions: u32,
    links: Vec<i32>,
    is_keyring: bool,
    negative: bool,
    revoked: bool,
    invalidated: bool,
    timeout_sec: u32,
    expires_at_sec: u64,
    quota_bytes: usize,
}

#[derive(Clone, Debug, Default)]
struct KeyUserQuota {
    keys: usize,
    bytes: usize,
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

fn capability_header_target_pid(
    header: &LinuxCapHeader,
) -> Result<Option<ProcessID>, SyscallError> {
    if header.pid < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if header.pid == 0 {
        return Ok(None);
    }
    Ok(Some(ProcessID(header.pid as u64)))
}

fn capability_header_targets_current_process(
    header: &LinuxCapHeader,
) -> Result<bool, SyscallError> {
    let Some(pid) = capability_header_target_pid(header)? else {
        return Ok(true);
    };
    Ok(pid == get_current_process().lock().pid)
}

fn capability_data_for_process(process: &ProcessRef) -> [LinuxCapData; LINUX_CAPABILITY_U32S_3] {
    let process = process.lock();
    core::array::from_fn(|index| LinuxCapData {
        effective: process.capability_effective[index],
        permitted: process.capability_permitted[index],
        inheritable: process.capability_inheritable[index],
    })
}

fn current_capability_data() -> [LinuxCapData; LINUX_CAPABILITY_U32S_3] {
    capability_data_for_process(&get_current_process())
}

fn capability_u32s(version: u32) -> Option<usize> {
    match version {
        LINUX_CAPABILITY_VERSION_1 => Some(LINUX_CAPABILITY_U32S_1),
        LINUX_CAPABILITY_VERSION_2 | LINUX_CAPABILITY_VERSION_3 => Some(LINUX_CAPABILITY_U32S_3),
        _ => None,
    }
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

fn has_capability_bits(caps: &[u32; LINUX_CAPABILITY_U32S_3], capability: usize) -> bool {
    let slot = capability / 32;
    let mask = 1u32 << (capability % 32);
    caps[slot] & mask != 0
}

fn validate_capset_data(
    new_data: &[LinuxCapData; LINUX_CAPABILITY_U32S_3],
) -> Result<(), SyscallError> {
    let process = get_current_process();
    let process = process.lock();
    let has_setpcap = has_capability_bits(&process.capability_effective, CAP_SETPCAP);

    for (index, caps) in new_data.iter().enumerate() {
        if caps.effective & !caps.permitted != 0 {
            return Err(SyscallError::PermissionDenied);
        }
        if caps.permitted & !process.capability_permitted[index] != 0 {
            return Err(SyscallError::PermissionDenied);
        }

        let allowed_inheritable = if has_setpcap {
            process.capability_inheritable[index]
                | (process.capability_permitted[index] & process.capability_bounding[index])
        } else {
            process.capability_inheritable[index]
        };
        if caps.inheritable & !allowed_inheritable != 0 {
            return Err(SyscallError::PermissionDenied);
        }
    }

    Ok(())
}

fn next_keyring_id() -> i32 {
    NEXT_SESSION_KEYRING_ID.fetch_add(1, Ordering::Relaxed)
}

fn current_thread_keyring(create: bool) -> Result<i32, SyscallError> {
    let process = get_current_process();
    let (serial, uid, gid, created) = {
        let mut process = process.lock();
        if process.thread_keyring == 0 {
            if !create {
                return Err(SyscallError::NoData);
            }
            process.thread_keyring = next_keyring_id();
            (
                process.thread_keyring,
                process.effective_uid,
                process.effective_gid,
                true,
            )
        } else {
            (
                process.thread_keyring,
                process.effective_uid,
                process.effective_gid,
                false,
            )
        }
    };
    if created {
        ensure_keyring_entry_with_owner(serial, "", uid, gid);
    }
    Ok(serial)
}

fn current_process_keyring(create: bool) -> Result<i32, SyscallError> {
    let process = get_current_process();
    let (serial, uid, gid, created) = {
        let mut process = process.lock();
        if process.process_keyring == 0 {
            if !create {
                return Err(SyscallError::NoData);
            }
            process.process_keyring = next_keyring_id();
            (
                process.process_keyring,
                process.effective_uid,
                process.effective_gid,
                true,
            )
        } else {
            (
                process.process_keyring,
                process.effective_uid,
                process.effective_gid,
                false,
            )
        }
    };
    if created {
        ensure_keyring_entry_with_owner(serial, "", uid, gid);
    }
    Ok(serial)
}

fn current_session_keyring(create: bool) -> Result<i32, SyscallError> {
    let process = get_current_process();
    let (serial, uid, gid, created) = {
        let mut process = process.lock();
        if process.session_keyring == 0 {
            if !create {
                return Err(SyscallError::NoData);
            }
            process.session_keyring = next_keyring_id();
            (
                process.session_keyring,
                process.effective_uid,
                process.effective_gid,
                true,
            )
        } else {
            (
                process.session_keyring,
                process.effective_uid,
                process.effective_gid,
                false,
            )
        }
    };
    if created {
        ensure_keyring_entry_with_owner(serial, "", uid, gid);
    }
    Ok(serial)
}

fn current_user_keyring(kind: UserKeyringKind, create: bool) -> Result<i32, SyscallError> {
    let (uid, gid) = current_key_owner();
    {
        let user_keyrings = USER_KEYRINGS.lock();
        if let Some(serial) = user_keyrings.get(&(uid, kind)) {
            return Ok(*serial);
        }
    }
    if !create {
        return Err(SyscallError::NoData);
    }

    let serial = next_keyring_id();
    let description = match kind {
        UserKeyringKind::User => alloc::format!("_uid.{uid}"),
        UserKeyringKind::UserSession => alloc::format!("_uid_ses.{uid}"),
    };
    ensure_keyring_entry_with_owner(serial, &description, uid, gid);
    USER_KEYRINGS.lock().insert((uid, kind), serial);
    Ok(serial)
}

fn resolve_keyring(spec: i32, create: bool) -> Result<i32, SyscallError> {
    match spec {
        KEY_SPEC_THREAD_KEYRING => current_thread_keyring(create),
        KEY_SPEC_PROCESS_KEYRING => current_process_keyring(create),
        KEY_SPEC_SESSION_KEYRING => current_session_keyring(create),
        KEY_SPEC_USER_KEYRING => current_user_keyring(UserKeyringKind::User, create),
        KEY_SPEC_USER_SESSION_KEYRING => current_user_keyring(UserKeyringKind::UserSession, true),
        serial if serial > 0 => {
            if create {
                ensure_keyring_entry(serial, "");
                Ok(serial)
            } else if keyring_exists(serial) {
                Ok(serial)
            } else {
                Err(SyscallError::NoKey)
            }
        }
        _ => Err(SyscallError::NoKey),
    }
}

fn resolve_key_serial(spec: i32) -> Result<i32, SyscallError> {
    match spec {
        KEY_SPEC_THREAD_KEYRING
        | KEY_SPEC_PROCESS_KEYRING
        | KEY_SPEC_SESSION_KEYRING
        | KEY_SPEC_USER_KEYRING
        | KEY_SPEC_USER_SESSION_KEYRING => resolve_keyring(spec, true),
        serial if serial > 0 => Ok(serial),
        _ => Err(SyscallError::NoKey),
    }
}

fn resolve_existing_keyring(spec: i32) -> Result<i32, SyscallError> {
    match spec {
        KEY_SPEC_THREAD_KEYRING
        | KEY_SPEC_PROCESS_KEYRING
        | KEY_SPEC_SESSION_KEYRING
        | KEY_SPEC_USER_KEYRING
        | KEY_SPEC_USER_SESSION_KEYRING => resolve_keyring(spec, false),
        serial if serial > 0 => {
            let registry = KEY_REGISTRY.lock();
            let entry = registry.get(&serial).ok_or(SyscallError::NoKey)?;
            if entry.is_keyring {
                Ok(serial)
            } else {
                Err(SyscallError::InvalidArguments)
            }
        }
        _ => Err(SyscallError::NoKey),
    }
}

fn keyring_exists(serial: i32) -> bool {
    KEY_REGISTRY
        .lock()
        .get(&serial)
        .is_some_and(|entry| entry.is_keyring && !entry.revoked)
}

fn ensure_keyring_entry(serial: i32, description: &str) {
    let (uid, gid) = current_key_owner();
    ensure_keyring_entry_with_owner(serial, description, uid, gid);
}

fn ensure_keyring_entry_with_owner(serial: i32, description: &str, uid: u32, gid: u32) {
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry.entry(serial).or_default();
    if !entry.is_keyring {
        entry.permissions = 0x3f3f_0000;
    }
    entry.type_name = "keyring".into();
    entry.is_keyring = true;
    entry.uid = uid;
    entry.gid = gid;
    if !description.is_empty() {
        entry.description = description.into();
    }
}

fn ensure_key_entry(serial: i32, type_name: &str, description: &str, payload: Vec<u8>) {
    let (uid, gid) = current_key_owner();
    let quota_bytes = key_user_payload_bytes(description, payload.len());
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry.entry(serial).or_default();
    entry.type_name = type_name.into();
    entry.description = description.into();
    entry.payload = payload;
    entry.uid = uid;
    entry.gid = gid;
    entry.permissions = 0x3f3f_0000;
    entry.is_keyring = false;
    entry.negative = false;
    entry.quota_bytes = quota_bytes;
}

fn ensure_negative_key_entry(serial: i32, type_name: &str, description: &str) {
    let (uid, gid) = current_key_owner();
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry.entry(serial).or_default();
    entry.type_name = type_name.into();
    entry.description = description.into();
    entry.payload.clear();
    entry.uid = uid;
    entry.gid = gid;
    entry.permissions = 0x3f3f_0000;
    entry.is_keyring = false;
    entry.negative = true;
    entry.quota_bytes = 0;
}

fn key_user_payload_bytes(description: &str, plen: usize) -> usize {
    description.len().saturating_add(1).saturating_add(plen)
}

fn reserve_user_key_quota(uid: u32, description: &str, plen: usize) -> Result<(), SyscallError> {
    let bytes = key_user_payload_bytes(description, plen);
    let mut quotas = KEY_USER_QUOTAS.lock();
    let quota = quotas.entry(uid).or_default();
    let (max_keys, max_bytes) = crate::filesystem::procfs::proc_keys_quota_limits(uid);
    if quota.keys.saturating_add(1) > max_keys || quota.bytes.saturating_add(bytes) > max_bytes {
        return Err(SyscallError::QuotaExceeded);
    }

    quota.keys += 1;
    quota.bytes += bytes;
    Ok(())
}

pub(crate) fn proc_key_users_bytes() -> Vec<u8> {
    let quotas = KEY_USER_QUOTAS.lock();
    let mut out = String::new();
    for (uid, quota) in quotas.iter() {
        let (max_keys, max_bytes) = crate::filesystem::procfs::proc_keys_quota_limits(*uid);
        out.push_str(&alloc::format!(
            "{uid:5}: {:5} {}/{} {}/{} {}/{}\n",
            0,
            quota.keys,
            quota.keys,
            quota.keys,
            max_keys,
            quota.bytes,
            max_bytes,
        ));
    }
    out.into_bytes()
}

fn release_key_quota(entry: &KeyEntry) {
    if entry.quota_bytes == 0 {
        return;
    }
    let mut quotas = KEY_USER_QUOTAS.lock();
    let Some(quota) = quotas.get_mut(&entry.uid) else {
        return;
    };
    quota.keys = quota.keys.saturating_sub(1);
    quota.bytes = quota.bytes.saturating_sub(entry.quota_bytes);
}

fn set_key_permissions(serial: i32, permissions: u32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry.get_mut(&serial).ok_or(SyscallError::NoKey)?;
    if entry.revoked {
        return Err(SyscallError::KeyRevoked);
    }
    entry.permissions = permissions;
    Ok(())
}

fn link_key_into_keyring(source: i32, target: i32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let Some(source_entry) = registry.get(&source).cloned() else {
        return Err(SyscallError::NoKey);
    };
    if source_entry.revoked {
        return Err(SyscallError::KeyRevoked);
    }
    let target_entry = registry.get(&target).ok_or(SyscallError::NoKey)?;
    if !target_entry.is_keyring || target_entry.revoked {
        return Err(SyscallError::NoKey);
    }
    let old_serial = target_entry.links.iter().copied().find(|serial| {
        registry.get(serial).is_some_and(|entry| {
            entry.type_name == source_entry.type_name
                && entry.description == source_entry.description
                && !entry.is_keyring
        })
    });
    if let Some(old_serial) = old_serial
        && old_serial != source
    {
        if let Some(old_entry) = registry.remove(&old_serial) {
            release_key_quota(&old_entry);
        }
        remove_key_from_all_keyrings(&mut registry, old_serial);
    }

    let target_entry = registry.get_mut(&target).ok_or(SyscallError::NoKey)?;
    if !target_entry.links.contains(&source) {
        target_entry.links.push(source);
    }
    Ok(())
}

fn revoke_key(serial: i32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let Some(entry) = registry.get(&serial) else {
        return Err(SyscallError::NoKey);
    };
    if !entry.is_keyring {
        let entry = registry.remove(&serial).ok_or(SyscallError::NoKey)?;
        release_key_quota(&entry);
        remove_key_from_all_keyrings(&mut registry, serial);
        return Ok(());
    }

    let entry = registry.get_mut(&serial).ok_or(SyscallError::NoKey)?;
    entry.revoked = true;
    entry.links.clear();
    Ok(())
}

fn current_key_owner() -> (u32, u32) {
    let process = get_current_process();
    let process = process.lock();
    (process.effective_uid, process.effective_gid)
}

fn get_live_key(serial: i32) -> Result<KeyEntry, SyscallError> {
    let registry = KEY_REGISTRY.lock();
    let entry = registry.get(&serial).ok_or(SyscallError::NoKey)?;
    if entry.revoked {
        return Err(SyscallError::KeyRevoked);
    }
    if entry.invalidated {
        return Err(SyscallError::NoKey);
    }
    if entry.negative {
        return Err(SyscallError::NoKey);
    }
    if entry.expires_at_sec != 0 && KernelTime::current().as_seconds() >= entry.expires_at_sec {
        return Err(SyscallError::KeyExpired);
    }
    Ok(entry.clone())
}

fn update_key_payload(serial: i32, payload: *const u8, plen: usize) -> Result<(), SyscallError> {
    let next_payload = user_safe::read_buffer(payload, plen)?;
    let mut registry = KEY_REGISTRY.lock();
    let current = registry.get(&serial).ok_or(SyscallError::NoKey)?.clone();
    if current.revoked {
        return Err(SyscallError::KeyRevoked);
    }
    if current.is_keyring {
        return Err(SyscallError::InvalidArguments);
    }
    if current.type_name == "user" {
        if plen > KEY_USER_MAX_PAYLOAD {
            return Err(SyscallError::InvalidArguments);
        }
        release_key_quota(&current);
        if let Err(err) = reserve_user_key_quota(current.uid, &current.description, plen) {
            let _ =
                reserve_user_key_quota(current.uid, &current.description, current.payload.len());
            return Err(err);
        }
    }
    let entry = registry.get_mut(&serial).ok_or(SyscallError::NoKey)?;
    if entry.revoked {
        return Err(SyscallError::KeyRevoked);
    }
    if entry.is_keyring {
        return Err(SyscallError::InvalidArguments);
    }
    entry.quota_bytes = key_user_payload_bytes(&entry.description, plen);
    entry.payload = next_payload;
    Ok(())
}

fn chown_key(serial: i32, uid: u32, gid: u32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry.get_mut(&serial).ok_or(SyscallError::NoKey)?;
    if entry.revoked {
        return Err(SyscallError::KeyRevoked);
    }
    entry.uid = uid;
    entry.gid = gid;
    Ok(())
}

fn clear_keyring(serial: i32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry.get_mut(&serial).ok_or(SyscallError::NoKey)?;
    if !entry.is_keyring {
        return Err(SyscallError::InvalidArguments);
    }
    if entry.revoked {
        return Err(SyscallError::KeyRevoked);
    }
    entry.links.clear();
    Ok(())
}

fn unlink_key_from_keyring(source: i32, target: i32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    if !registry.contains_key(&source) {
        return Err(SyscallError::NoKey);
    }
    let entry = registry.get_mut(&target).ok_or(SyscallError::NoKey)?;
    if !entry.is_keyring {
        return Err(SyscallError::InvalidArguments);
    }
    entry.links.retain(|link| *link != source);
    Ok(())
}

fn remove_key_from_all_keyrings(registry: &mut BTreeMap<i32, KeyEntry>, serial: i32) {
    for entry in registry.values_mut() {
        if entry.is_keyring {
            entry.links.retain(|link| *link != serial);
        }
    }
}

fn search_keyring(keyring: i32, type_name: &str, description: &str) -> Result<i32, SyscallError> {
    let registry = KEY_REGISTRY.lock();
    let entry = registry.get(&keyring).ok_or(SyscallError::NoKey)?;
    if !entry.is_keyring {
        return Err(SyscallError::InvalidArguments);
    }
    if entry.revoked {
        return Err(SyscallError::KeyRevoked);
    }
    if entry.expires_at_sec != 0 && KernelTime::current().as_seconds() >= entry.expires_at_sec {
        return Err(SyscallError::KeyExpired);
    }
    for serial in &entry.links {
        let Some(linked) = registry.get(serial) else {
            continue;
        };
        if !linked.revoked
            && !linked.invalidated
            && !linked.negative
            && linked.type_name == type_name
            && linked.description == description
        {
            return Ok(*serial);
        }
    }
    for serial in &entry.links {
        let Some(linked) = registry.get(serial) else {
            continue;
        };
        if linked.type_name == type_name && linked.description == description {
            if linked.revoked {
                return Err(SyscallError::KeyRevoked);
            }
            if linked.expires_at_sec != 0
                && KernelTime::current().as_seconds() >= linked.expires_at_sec
            {
                return Err(SyscallError::KeyExpired);
            }
            if linked.invalidated {
                return Err(SyscallError::NoKey);
            }
            if linked.negative {
                return Err(SyscallError::NoKey);
            }
        }
    }
    Err(SyscallError::NoKey)
}

fn keyring_allows_write(serial: i32) -> Result<bool, SyscallError> {
    let entry = get_live_key(serial)?;
    if !entry.is_keyring {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(entry.permissions & KEY_POS_WRITE != 0)
}

fn describe_key(serial: i32) -> Result<Vec<u8>, SyscallError> {
    let entry = get_live_key(serial)?;
    Ok(alloc::format!(
        "{};{};{};{:08x};{}",
        entry.type_name,
        entry.uid,
        entry.gid,
        entry.permissions,
        entry.description,
    )
    .into_bytes())
}

fn read_key(serial: i32) -> Result<Vec<u8>, SyscallError> {
    let entry = get_live_key(serial)?;
    if entry.is_keyring {
        let mut out = Vec::with_capacity(entry.links.len() * core::mem::size_of::<i32>());
        for link in entry.links {
            out.extend_from_slice(&link.to_ne_bytes());
        }
        Ok(out)
    } else {
        Ok(entry.payload)
    }
}

fn copy_keyctl_bytes_to_user(
    bytes: &[u8],
    buffer: *mut u8,
    buflen: usize,
) -> Result<usize, SyscallError> {
    if buflen == 0 {
        return Ok(bytes.len());
    }
    if buffer.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let copied = core::cmp::min(buflen, bytes.len());
    user_safe::write_buffer(buffer, &bytes[..copied])?;
    Ok(bytes.len())
}

fn invalidate_key(serial: i32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let Some(entry) = registry.remove(&serial) else {
        return Err(SyscallError::NoKey);
    };
    release_key_quota(&entry);
    remove_key_from_all_keyrings(&mut registry, serial);
    Ok(())
}

fn set_key_timeout(serial: i32, timeout_sec: u32) -> Result<(), SyscallError> {
    let mut registry = KEY_REGISTRY.lock();
    let entry = registry.get_mut(&serial).ok_or(SyscallError::NoKey)?;
    if entry.revoked {
        return Err(SyscallError::KeyRevoked);
    }
    entry.timeout_sec = timeout_sec;
    entry.expires_at_sec = if timeout_sec == 0 {
        0
    } else {
        KernelTime::current()
            .as_seconds()
            .saturating_add(timeout_sec as u64)
    };
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
            | CloneFlags::PARENT.bits()
            | CloneFlags::FS.bits()
            | CloneFlags::FILES.bits()
            | CloneFlags::SIGHAND.bits()
            | CloneFlags::NEWNS.bits()
            | CloneFlags::SYSVSEM.bits()
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
    if unsupported != 0 {
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
    let share_addrspace = clone_flags.contains(CloneFlags::VM);
    if share_addrspace {
        prefault_clone_vm_tid_pages(clone_flags, parent_tid, child_tid)?;
    }
    let share_fd_table = clone_flags.contains(CloneFlags::FILES);
    let share_fs_context = clone_flags.contains(CloneFlags::FS);
    let (child_process, child_thread) = if is_vfork {
        Process::vfork_with_sharing(
            current.clone(),
            share_fd_table,
            share_fs_context,
            share_addrspace,
        )
    } else {
        Process::fork_with_sharing(
            current.clone(),
            share_fd_table,
            share_fs_context,
            share_addrspace,
        )
    };
    if clone_flags.contains(CloneFlags::NEWNET) {
        child_process.lock().net_namespace = NetNamespace::new();
    }
    if clone_flags.contains(CloneFlags::NEWNS) {
        child_process.lock().mnt_namespace = NamespaceObject::dynamic(NamespaceKind::Mnt);
    }
    if clone_flags.contains(CloneFlags::NEWUTS) {
        child_process.lock().uts_namespace = NamespaceObject::dynamic(NamespaceKind::Uts);
    }
    if clone_flags.contains(CloneFlags::NEWIPC) {
        child_process.lock().ipc_namespace = NamespaceObject::dynamic(NamespaceKind::Ipc);
    }
    if clone_flags.contains(CloneFlags::PARENT) {
        let parent = current.lock().parent.clone();
        child_process.lock().parent = parent;
    }
    if exit_signal != 0 {
        child_process.lock().child_exit_signal =
            Signal::try_from(u64::from(exit_signal)).map_err(|_| SyscallError::InvalidArguments)?;
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
        if share_addrspace {
            child_process
                .lock()
                .addrspace
                .write(parent_tid, &(pid.0 as i32))?;
        }
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

    Process::wake_vfork_child(child_thread);

    if is_vfork {
        // Keep vfork safe for multi-threaded parents by using the existing
        // fork/COW address-space clone and only adding the parent wait semantics.
        wait_for_vfork_completion(&child_process);
    }

    Ok(pid.0 as usize)
}

fn prefault_clone_vm_tid_pages(
    clone_flags: CloneFlags,
    parent_tid: *mut i32,
    child_tid: *mut i32,
) -> Result<(), SyscallError> {
    if clone_flags.contains(CloneFlags::PARENT_SETTID) {
        let _: i32 = user_safe::read(parent_tid.cast_const())?;
    }
    if clone_flags.contains(CloneFlags::CHILD_SETTID)
        || clone_flags.contains(CloneFlags::CHILD_CLEARTID)
    {
        let _: i32 = user_safe::read(child_tid.cast_const())?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
enum RlimitResource {
    Cpu = 0,
    Fsize = 1,
    Data = 2,
    Rss = 5,
    Nproc = 6,
    Core = 4,
    Stack = 3,
    NoFile = 7,
    MemLock = 8,
    As = 9,
    Locks = 10,
    Sigpending = 11,
    Msgqueue = 12,
    Nice = 13,
    RtPrio = 14,
    Rttime = 15,
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
