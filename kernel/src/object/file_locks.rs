use alloc::{collections::BTreeMap, format, string::String, sync::Arc, vec::Vec};

use lazy_static::lazy_static;
use spin::Mutex;

use crate::{
    memory::user_safe,
    object::misc::ObjectRef,
    process::{FdEntry, manager::get_current_process, misc::ProcessID},
    systemcall::utils::{SyscallError, SyscallResult},
    thread::{THREAD_MANAGER, yielding::{BlockType, WakeType, block_current_with_sig_check}},
};

pub(crate) const F_RDLCK: i16 = 0;
pub(crate) const F_WRLCK: i16 = 1;
pub(crate) const F_UNLCK: i16 = 2;
const SEEK_SET: i16 = 0;

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
enum AdvisoryLockType {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdvisoryLockOwner {
    Process(ProcessID),
    OpenFileDescription(usize),
}

#[derive(Clone, Copy, Debug)]
struct AdvisoryLock {
    owner: AdvisoryLockOwner,
    lock_type: AdvisoryLockType,
}

lazy_static! {
    static ref ADVISORY_LOCKS: Mutex<BTreeMap<String, Vec<AdvisoryLock>>> =
        Mutex::new(BTreeMap::new());
}

pub(crate) fn fcntl_get_lock(object: &ObjectRef, arg: *mut LinuxFlock, ofd: bool) -> SyscallResult {
    if arg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut flock = user_safe::read(arg)?;
    let requested_type = parse_flock_request(&flock)?;
    let owner = lock_owner(object, ofd);
    let key = lock_key(object)?;
    let conflict = {
        let locks = ADVISORY_LOCKS.lock();
        locks
            .get(&key)
            .and_then(|entries| find_conflict(entries, owner, requested_type))
    };

    if let Some(lock) = conflict {
        flock.lock_type = match lock.lock_type {
            AdvisoryLockType::Read => F_RDLCK,
            AdvisoryLockType::Write => F_WRLCK,
        };
        flock.pid = match lock.owner {
            AdvisoryLockOwner::Process(pid) => pid.0 as i32,
            AdvisoryLockOwner::OpenFileDescription(_) => -1,
        };
    } else {
        flock.lock_type = F_UNLCK;
        flock.pid = 0;
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
    let requested_type = parse_flock_request(&flock)?;
    let owner = lock_owner(object, ofd);
    let key = lock_key(object)?;

    loop {
        let mut changed = false;
        let conflict = {
            let mut locks = ADVISORY_LOCKS.lock();
            let entries = locks.entry(key.clone()).or_default();
            if let Some(conflict) = find_conflict(entries, owner, requested_type) {
                Some(conflict)
            } else {
                entries.retain(|entry| entry.owner != owner);
                if let Some(lock_type) = requested_type {
                    entries.push(AdvisoryLock { owner, lock_type });
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

fn parse_flock_request(flock: &LinuxFlock) -> Result<Option<AdvisoryLockType>, SyscallError> {
    if flock.whence != SEEK_SET || flock.start != 0 || flock.len != 0 {
        return Err(SyscallError::OperationNotSupported);
    }

    match flock.lock_type {
        F_RDLCK => Ok(Some(AdvisoryLockType::Read)),
        F_WRLCK => Ok(Some(AdvisoryLockType::Write)),
        F_UNLCK => Ok(None),
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

fn find_conflict(
    entries: &[AdvisoryLock],
    owner: AdvisoryLockOwner,
    requested_type: Option<AdvisoryLockType>,
) -> Option<AdvisoryLock> {
    let requested_type = requested_type?;
    entries.iter().copied().find(|entry| {
        if entry.owner == owner {
            return false;
        }
        !matches!(
            (requested_type, entry.lock_type),
            (AdvisoryLockType::Read, AdvisoryLockType::Read)
        )
    })
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
    THREAD_MANAGER.get().unwrap().lock().wake_io();
}
