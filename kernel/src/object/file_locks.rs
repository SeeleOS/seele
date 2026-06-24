use alloc::{collections::BTreeMap, format, string::String, sync::Arc, vec, vec::Vec};

use crate::memory::utils::Mut;
use lazy_static::lazy_static;

use crate::{
    memory::user_safe,
    object::misc::ObjectRef,
    process::{FdEntry, manager::get_current_process, misc::ProcessID},
    systemcall::utils::{SyscallError, SyscallResult},
    thread::yielding::{BlockType, WakeType, block_current_with_sig_check},
};

pub(crate) const F_RDLCK: i16 = 0;
pub(crate) const F_WRLCK: i16 = 1;
pub(crate) const F_UNLCK: i16 = 2;
const SEEK_SET: i16 = 0;
const LOCK_SH: i32 = 1;
const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct LinuxFlock {
    pub(crate) lock_type: i16,
    pub(crate) whence: i16,
    pub(crate) start: i64,
    pub(crate) len: i64,
    pub(crate) pid: i32,
    pub(crate) __reserved: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AdvisoryLockType {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AdvisoryLockApi {
    Posix,
    Flock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AdvisoryLockOwner {
    Process(ProcessID),
    OpenFileDescription(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AdvisoryLockRange {
    pub(super) start: u64,
    pub(super) end: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AdvisoryLock {
    pub(super) api: AdvisoryLockApi,
    pub(super) owner: AdvisoryLockOwner,
    pub(super) lock_type: AdvisoryLockType,
    pub(super) range: AdvisoryLockRange,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ParsedFlockRequest {
    pub(super) lock_type: Option<AdvisoryLockType>,
    pub(super) range: AdvisoryLockRange,
}

lazy_static! {
    static ref ADVISORY_LOCKS: Mut<BTreeMap<String, Vec<AdvisoryLock>>> = Mut::new(BTreeMap::new());
}

pub(crate) fn fcntl_get_lock(object: &ObjectRef, arg: *mut LinuxFlock, ofd: bool) -> SyscallResult {
    if arg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut flock = user_safe::read(arg)?;
    let requested = parse_flock_request(&flock, object)?;
    let owner = lock_owner(object, ofd);
    let key = lock_key(object)?;
    let conflict = {
        let locks = ADVISORY_LOCKS.lock();
        locks.get(&key).and_then(|entries| {
            find_conflict(
                entries,
                owner,
                requested.lock_type.map(|lock_type| AdvisoryLock {
                    api: AdvisoryLockApi::Posix,
                    owner,
                    lock_type,
                    range: requested.range,
                }),
                AdvisoryLockApi::Posix,
            )
        })
    };

    if let Some(lock) = conflict {
        flock.lock_type = match lock.lock_type {
            AdvisoryLockType::Read => F_RDLCK,
            AdvisoryLockType::Write => F_WRLCK,
        };
        flock.whence = SEEK_SET;
        flock.start = lock.range.start as i64;
        flock.len = lock
            .range
            .end
            .map(|end| end.saturating_sub(lock.range.start) as i64)
            .unwrap_or(0);
        flock.pid = match lock.owner {
            AdvisoryLockOwner::Process(pid) => pid.0 as i32,
            AdvisoryLockOwner::OpenFileDescription(_) => -1,
        };
    } else {
        flock.lock_type = F_UNLCK;
    }

    user_safe::write(arg, &flock)?;
    Ok(0)
}

pub(crate) fn fcntl_set_lock(
    object: &ObjectRef,
    arg: *mut LinuxFlock,
    ofd: bool,
    blocking: bool,
) -> SyscallResult {
    if arg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let flock = user_safe::read(arg)?;
    let requested = parse_flock_request(&flock, object)?;
    let owner = lock_owner(object, ofd);
    let key = lock_key(object)?;

    loop {
        let mut changed = false;
        let conflict = {
            let mut locks = ADVISORY_LOCKS.lock();
            let entries = locks.entry(key.clone()).or_default();
            if let Some(conflict) = find_conflict(
                entries,
                owner,
                requested.lock_type.map(|lock_type| AdvisoryLock {
                    api: AdvisoryLockApi::Posix,
                    owner,
                    lock_type,
                    range: requested.range,
                }),
                AdvisoryLockApi::Posix,
            ) {
                Some(conflict)
            } else {
                apply_posix_lock(entries, owner, requested);
                changed = true;
                if entries.is_empty() {
                    locks.remove(&key);
                }
                None
            }
        };

        if conflict.is_none() {
            if changed {
                wake_lock_waiters();
            }
            return Ok(0);
        }
        if !blocking {
            return Err(SyscallError::TryAgain);
        }

        block_current_with_sig_check(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline: None,
        })?;
    }
}

pub(crate) fn flock_lock(object: &ObjectRef, operation: i32) -> SyscallResult {
    let requested_type = parse_flock_operation(operation)?;
    let owner = AdvisoryLockOwner::OpenFileDescription(ofd_owner(object));
    let key = lock_key(object)?;
    let blocking = operation & LOCK_NB == 0;
    let requested_type = requested_type.map(|lock_type| AdvisoryLock {
        api: AdvisoryLockApi::Flock,
        owner,
        lock_type,
        range: AdvisoryLockRange {
            start: 0,
            end: None,
        },
    });

    loop {
        let mut changed = false;
        let conflict = {
            let mut locks = ADVISORY_LOCKS.lock();
            let entries = locks.entry(key.clone()).or_default();
            if let Some(conflict) =
                find_conflict(entries, owner, requested_type, AdvisoryLockApi::Flock)
            {
                Some(conflict)
            } else {
                entries
                    .retain(|entry| !(entry.api == AdvisoryLockApi::Flock && entry.owner == owner));
                if let Some(lock) = requested_type {
                    entries.push(lock);
                }
                changed = true;
                if entries.is_empty() {
                    locks.remove(&key);
                }
                None
            }
        };

        if conflict.is_none() {
            if changed {
                wake_lock_waiters();
            }
            return Ok(0);
        }
        if !blocking {
            return Err(SyscallError::TryAgain);
        }

        block_current_with_sig_check(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline: None,
        })?;
    }
}

pub(crate) fn release_fd_entry_locks(process_pid: ProcessID, entry: &FdEntry) {
    let Ok(key) = lock_key(&entry.object) else {
        return;
    };

    let mut changed = release_process_lock(&key, process_pid);
    if Arc::strong_count(&entry.object) == 1 {
        changed |= release_ofd_lock(&key, ofd_owner(&entry.object));
    }
    if changed {
        wake_lock_waiters();
    }
}

pub(crate) fn release_process_fd_table_locks(process_pid: ProcessID, entries: &[Option<FdEntry>]) {
    let mut changed = false;
    for entry in entries.iter().flatten() {
        let Ok(key) = lock_key(&entry.object) else {
            continue;
        };
        changed |= release_process_lock(&key, process_pid);
    }
    if changed {
        wake_lock_waiters();
    }
}

fn parse_flock_request(
    flock: &LinuxFlock,
    object: &ObjectRef,
) -> Result<ParsedFlockRequest, SyscallError> {
    let range = resolve_flock_range(flock, object)?;

    let lock_type = match flock.lock_type {
        F_RDLCK => Some(AdvisoryLockType::Read),
        F_WRLCK => Some(AdvisoryLockType::Write),
        F_UNLCK => None,
        _ => return Err(SyscallError::InvalidArguments),
    };

    Ok(ParsedFlockRequest { lock_type, range })
}

pub(super) fn parse_flock_operation(
    operation: i32,
) -> Result<Option<AdvisoryLockType>, SyscallError> {
    let mode = operation & (LOCK_SH | LOCK_EX | LOCK_UN);
    let extra = operation & !(LOCK_SH | LOCK_EX | LOCK_UN | LOCK_NB);
    if extra != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    match mode {
        LOCK_SH => Ok(Some(AdvisoryLockType::Read)),
        LOCK_EX => Ok(Some(AdvisoryLockType::Write)),
        LOCK_UN => Ok(None),
        _ => Err(SyscallError::InvalidArguments),
    }
}

fn lock_owner(object: &ObjectRef, ofd: bool) -> AdvisoryLockOwner {
    if ofd {
        AdvisoryLockOwner::OpenFileDescription(ofd_owner(object))
    } else {
        AdvisoryLockOwner::Process(get_current_process().lock().pid)
    }
}

fn ofd_owner(object: &ObjectRef) -> usize {
    Arc::as_ptr(object) as *const () as usize
}

fn lock_key(object: &ObjectRef) -> Result<String, SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        return Ok(file_like.path().as_string());
    }

    Ok(format!("object:{:p}", Arc::as_ptr(object)))
}

pub(super) fn find_conflict(
    entries: &[AdvisoryLock],
    owner: AdvisoryLockOwner,
    requested_lock: Option<AdvisoryLock>,
    api: AdvisoryLockApi,
) -> Option<AdvisoryLock> {
    let requested_lock = requested_lock?;
    entries
        .iter()
        .copied()
        .filter(|entry| {
            if entry.api != api {
                return false;
            }
            if entry.owner == owner {
                return false;
            }
            if !ranges_overlap(entry.range, requested_lock.range) {
                return false;
            }
            !matches!(
                (requested_lock.lock_type, entry.lock_type),
                (AdvisoryLockType::Read, AdvisoryLockType::Read)
            )
        })
        .min_by_key(|entry| (entry.range.start, range_end_bound(entry.range)))
}

fn resolve_flock_range(
    flock: &LinuxFlock,
    object: &ObjectRef,
) -> Result<AdvisoryLockRange, SyscallError> {
    let base = match flock.whence {
        SEEK_SET => 0,
        1 => object
            .clone()
            .as_seekable()?
            .seek(0, crate::filesystem::vfs_traits::Whence::Current)? as u64,
        2 => {
            object
                .clone()
                .as_file_like()?
                .info()
                .map_err(SyscallError::from)?
                .size as u64
        }
        _ => return Err(SyscallError::InvalidArguments),
    };
    let start = base
        .checked_add_signed(flock.start)
        .ok_or(SyscallError::InvalidArguments)?;
    let end = if flock.len > 0 {
        Some(
            start
                .checked_add(flock.len as u64)
                .ok_or(SyscallError::InvalidArguments)?,
        )
    } else if flock.len == 0 {
        None
    } else {
        let len = flock.len.unsigned_abs();
        let new_start = start
            .checked_sub(len)
            .ok_or(SyscallError::InvalidArguments)?;
        return Ok(AdvisoryLockRange {
            start: new_start,
            end: Some(start),
        });
    };

    Ok(AdvisoryLockRange { start, end })
}

pub(super) fn ranges_overlap(left: AdvisoryLockRange, right: AdvisoryLockRange) -> bool {
    left.start < range_end_bound(right) && right.start < range_end_bound(left)
}

fn range_end_bound(range: AdvisoryLockRange) -> u64 {
    range.end.unwrap_or(u64::MAX)
}

pub(super) fn apply_posix_lock(
    entries: &mut Vec<AdvisoryLock>,
    owner: AdvisoryLockOwner,
    request: ParsedFlockRequest,
) {
    let mut updated = Vec::with_capacity(entries.len() + usize::from(request.lock_type.is_some()));

    for entry in entries.iter().copied() {
        if entry.api != AdvisoryLockApi::Posix || entry.owner != owner {
            updated.push(entry);
            continue;
        }

        updated.extend(subtract_lock_range(entry, request.range));
    }

    if let Some(lock_type) = request.lock_type {
        updated.push(AdvisoryLock {
            api: AdvisoryLockApi::Posix,
            owner,
            lock_type,
            range: request.range,
        });
    }

    merge_posix_locks(&mut updated);
    *entries = updated;
}

fn subtract_lock_range(lock: AdvisoryLock, remove: AdvisoryLockRange) -> Vec<AdvisoryLock> {
    if !ranges_overlap(lock.range, remove) {
        return vec![lock];
    }

    let mut remaining = Vec::with_capacity(2);
    if lock.range.start < remove.start {
        remaining.push(AdvisoryLock {
            range: AdvisoryLockRange {
                start: lock.range.start,
                end: Some(remove.start),
            },
            ..lock
        });
    }

    let lock_end = lock.range.end;
    let remove_end = remove.end;
    if let Some(remove_end) = remove_end {
        let right_keeps_data = lock_end.is_none_or(|end| remove_end < end);
        if right_keeps_data {
            remaining.push(AdvisoryLock {
                range: AdvisoryLockRange {
                    start: remove_end,
                    end: lock_end,
                },
                ..lock
            });
        }
    }

    remaining
}

fn merge_posix_locks(entries: &mut Vec<AdvisoryLock>) {
    entries.sort_by_key(|entry| {
        (
            entry.api as u8,
            entry.owner.owner_sort_key(),
            entry.lock_type as u8,
            entry.range.start,
            range_end_bound(entry.range),
        )
    });

    let mut merged = Vec::with_capacity(entries.len());
    for entry in entries.drain(..) {
        let Some(last) = merged.last_mut() else {
            merged.push(entry);
            continue;
        };

        if can_merge_posix_locks(*last, entry) {
            last.range.end = merge_lock_ends(last.range.end, entry.range.end);
        } else {
            merged.push(entry);
        }
    }
    *entries = merged;
}

fn can_merge_posix_locks(left: AdvisoryLock, right: AdvisoryLock) -> bool {
    left.api == AdvisoryLockApi::Posix
        && right.api == AdvisoryLockApi::Posix
        && left.owner == right.owner
        && left.lock_type == right.lock_type
        && range_end_bound(left.range) >= right.range.start
}

fn merge_lock_ends(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, _) | (_, None) => None,
        (Some(left), Some(right)) => Some(left.max(right)),
    }
}

impl AdvisoryLockOwner {
    fn owner_sort_key(self) -> (u8, u64) {
        match self {
            Self::Process(pid) => (0, pid.0),
            Self::OpenFileDescription(owner) => (1, owner as u64),
        }
    }
}

fn release_process_lock(key: &str, process_pid: ProcessID) -> bool {
    let mut locks = ADVISORY_LOCKS.lock();
    let Some(entries) = locks.get_mut(key) else {
        return false;
    };
    let old_len = entries.len();
    entries.retain(|entry| entry.owner != AdvisoryLockOwner::Process(process_pid));
    let changed = entries.len() != old_len;
    if entries.is_empty() {
        locks.remove(key);
    }
    changed
}

fn release_ofd_lock(key: &str, owner: usize) -> bool {
    let mut locks = ADVISORY_LOCKS.lock();
    let Some(entries) = locks.get_mut(key) else {
        return false;
    };
    let old_len = entries.len();
    entries.retain(|entry| entry.owner != AdvisoryLockOwner::OpenFileDescription(owner));
    let changed = entries.len() != old_len;
    if entries.is_empty() {
        locks.remove(key);
    }
    changed
}

fn wake_lock_waiters() {
    crate::thread::with_thread_manager(|manager| manager.wake_io());
}
