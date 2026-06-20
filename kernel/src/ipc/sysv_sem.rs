use alloc::{collections::BTreeMap, vec, vec::Vec};
use core::mem;

use lazy_static::lazy_static;

use crate::{
    memory::{user_safe, utils::Mut},
    misc::time::{Time, unix_timestamp_seconds},
    process::Process,
    systemcall::utils::{SyscallError, SyscallResult},
    thread::yielding::{BlockType, WakeType, block_current_with_sig_check},
};

const IPC_PRIVATE: i32 = 0;
const IPC_CREAT: i32 = 0o1000;
const IPC_EXCL: i32 = 0o2000;
const IPC_NOWAIT: i16 = 0o4000;
const IPC_RMID: i32 = 0;
const IPC_SET: i32 = 1;
const IPC_STAT: i32 = 2;
const IPC_INFO: i32 = 3;
const SEM_STAT: i32 = 18;
const SEM_INFO: i32 = 19;
const GETPID: i32 = 11;
const GETVAL: i32 = 12;
const GETALL: i32 = 13;
const GETNCNT: i32 = 14;
const GETZCNT: i32 = 15;
const SETVAL: i32 = 16;
const SETALL: i32 = 17;
const IPC_MODE_MASK: i32 = 0o777;
const SEMVMX: u16 = 32767;
const SEMMNI: usize = 32000;
const SEMMSL: usize = 32000;
const SEMMNS: usize = 1024000000;
const SEMOPM: usize = 500;

lazy_static! {
    static ref SYSV_SEM_STATE: Mut<SysvSemState> = Mut::new(SysvSemState::default());
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxIpcPerm {
    pub __ipc_perm_key: i32,
    pub uid: u32,
    pub gid: u32,
    pub cuid: u32,
    pub cgid: u32,
    pub mode: u32,
    pub __ipc_perm_seq: i32,
    pub __pad1: i64,
    pub __pad2: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxSemidDs {
    pub sem_perm: LinuxIpcPerm,
    pub sem_otime: i64,
    pub sem_ctime: i64,
    pub sem_nsems: u64,
    pub __pad1: u64,
    pub __pad2: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxSeminfo {
    pub semmap: i32,
    pub semmni: i32,
    pub semmns: i32,
    pub semmnu: i32,
    pub semmsl: i32,
    pub semopm: i32,
    pub semume: i32,
    pub semusz: i32,
    pub semvmx: i32,
    pub semaem: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxSembuf {
    pub sem_num: u16,
    pub sem_op: i16,
    pub sem_flg: i16,
}

#[derive(Debug, Default)]
struct SysvSemState {
    next_semid: i32,
    sets: BTreeMap<i32, SysvSemSet>,
}

#[derive(Clone, Debug)]
struct SysvSemSet {
    key: i32,
    values: Vec<u16>,
    last_pid: i32,
    owner_uid: u32,
    owner_gid: u32,
    creator_uid: u32,
    creator_gid: u32,
    mode: u32,
    seq: i32,
    otime: i64,
    ctime: i64,
    removed: bool,
}

impl SysvSemState {
    fn next_semid(&mut self) -> i32 {
        self.next_semid += 1;
        self.next_semid
    }
}

fn now_seconds() -> i64 {
    unix_timestamp_seconds().min(i64::MAX as u64) as i64
}

fn seminfo() -> LinuxSeminfo {
    LinuxSeminfo {
        semmap: SEMMNS as i32,
        semmni: SEMMNI as i32,
        semmns: SEMMNS as i32,
        semmnu: SEMMNI as i32,
        semmsl: SEMMSL as i32,
        semopm: SEMOPM as i32,
        semume: SEMOPM as i32,
        semusz: mem::size_of::<LinuxSemidDs>() as i32,
        semvmx: SEMVMX as i32,
        semaem: SEMVMX as i32,
    }
}

fn has_access(set: &SysvSemSet, process: &Process, write: bool) -> bool {
    if process.effective_uid == 0 {
        return true;
    }

    let mut mask = 0o4u32;
    if write {
        mask |= 0o2;
    }
    let mode = set.mode;
    if process.effective_uid == set.owner_uid || process.effective_uid == set.creator_uid {
        return mode & (mask << 6) == (mask << 6);
    }
    if process.effective_gid == set.owner_gid
        || process.effective_gid == set.creator_gid
        || process
            .supplementary_groups
            .iter()
            .any(|gid| *gid == set.owner_gid || *gid == set.creator_gid)
    {
        return mode & (mask << 3) == (mask << 3);
    }
    mode & mask == mask
}

fn linux_semid_ds(set: &SysvSemSet) -> LinuxSemidDs {
    LinuxSemidDs {
        sem_perm: LinuxIpcPerm {
            __ipc_perm_key: set.key,
            uid: set.owner_uid,
            gid: set.owner_gid,
            cuid: set.creator_uid,
            cgid: set.creator_gid,
            mode: set.mode,
            __ipc_perm_seq: set.seq,
            __pad1: 0,
            __pad2: 0,
        },
        sem_otime: set.otime,
        sem_ctime: set.ctime,
        sem_nsems: set.values.len() as u64,
        __pad1: 0,
        __pad2: 0,
    }
}

fn read_ops(ops: *const LinuxSembuf, nsops: usize) -> Result<Vec<LinuxSembuf>, SyscallError> {
    if nsops == 0 || nsops > SEMOPM {
        return Err(SyscallError::InvalidArguments);
    }
    if ops.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut result = Vec::with_capacity(nsops);
    for index in 0..nsops {
        result.push(user_safe::read(unsafe { ops.add(index) })?);
    }
    Ok(result)
}

fn ops_ready(set: &SysvSemSet, ops: &[LinuxSembuf]) -> Result<bool, SyscallError> {
    for op in ops {
        let index = op.sem_num as usize;
        let Some(&value) = set.values.get(index) else {
            return Err(SyscallError::FileTooLarge);
        };
        if op.sem_op < 0 {
            if value < op.sem_op.unsigned_abs() {
                return Ok(false);
            }
        } else if op.sem_op == 0 && value != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn apply_ops(set: &mut SysvSemSet, ops: &[LinuxSembuf], pid: i32) -> Result<(), SyscallError> {
    for op in ops {
        let value = set
            .values
            .get_mut(op.sem_num as usize)
            .ok_or(SyscallError::FileTooLarge)?;
        if op.sem_op < 0 {
            *value = value
                .checked_sub(op.sem_op.unsigned_abs())
                .ok_or(SyscallError::TryAgain)?;
        } else {
            *value = value
                .checked_add(op.sem_op as u16)
                .filter(|value| *value <= SEMVMX)
                .ok_or(SyscallError::RangeError)?;
        }
    }
    set.last_pid = pid;
    set.otime = now_seconds();
    Ok(())
}

fn timeout_deadline(timeout: *const LinuxTimespec) -> Result<Option<Time>, SyscallError> {
    if timeout.is_null() {
        return Ok(None);
    }
    let timeout = user_safe::read(timeout)?;
    if timeout.tv_sec < 0 || !(0..1_000_000_000).contains(&timeout.tv_nsec) {
        return Err(SyscallError::InvalidArguments);
    }
    let ns = (timeout.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(timeout.tv_nsec as u64);
    Ok(Some(Time::since_boot().add_ns(ns)))
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxTimespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

pub fn semget(process: &Process, key: i32, nsems: i32, semflg: i32) -> SyscallResult {
    let create = semflg & IPC_CREAT != 0;
    let exclusive = semflg & IPC_EXCL != 0;
    let nsems = usize::try_from(nsems).map_err(|_| SyscallError::InvalidArguments)?;
    if nsems > SEMMSL {
        return Err(SyscallError::InvalidArguments);
    }

    let mut state = SYSV_SEM_STATE.lock();
    if key != IPC_PRIVATE
        && let Some((semid, set)) = state
            .sets
            .iter()
            .find(|(_, set)| set.key == key && !set.removed)
    {
        if create && exclusive {
            return Err(SyscallError::FileAlreadyExists);
        }
        if nsems != 0 && nsems > set.values.len() {
            return Err(SyscallError::InvalidArguments);
        }
        if !has_access(set, process, false) {
            return Err(SyscallError::PermissionDenied);
        }
        return Ok(*semid as usize);
    }

    if !create && key != IPC_PRIVATE {
        return Err(SyscallError::FileNotFound);
    }
    if nsems == 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if state.sets.len() >= SEMMNI {
        return Err(SyscallError::NoSpaceLeft);
    }

    let semid = state.next_semid();
    let now = now_seconds();
    state.sets.insert(
        semid,
        SysvSemSet {
            key,
            values: vec![0; nsems],
            last_pid: process.pid.0 as i32,
            owner_uid: process.effective_uid,
            owner_gid: process.effective_gid,
            creator_uid: process.effective_uid,
            creator_gid: process.effective_gid,
            mode: (semflg & IPC_MODE_MASK) as u32,
            seq: 0,
            otime: 0,
            ctime: now,
            removed: false,
        },
    );
    Ok(semid as usize)
}

pub fn semctl(process: &Process, semid: i32, semnum: i32, cmd: i32, arg: usize) -> SyscallResult {
    match cmd {
        IPC_INFO | SEM_INFO => {
            if arg == 0 {
                return Err(SyscallError::BadAddress);
            }
            user_safe::write(arg as *mut LinuxSeminfo, &seminfo())?;
            return Ok(SEMMNI - 1);
        }
        _ => {}
    }

    let mut state = SYSV_SEM_STATE.lock();
    let set = state
        .sets
        .get_mut(&semid)
        .filter(|set| !set.removed)
        .ok_or(SyscallError::InvalidArguments)?;

    match cmd {
        IPC_RMID => {
            if process.effective_uid != 0
                && process.effective_uid != set.owner_uid
                && process.effective_uid != set.creator_uid
            {
                return Err(SyscallError::PermissionDenied);
            }
            set.removed = true;
            state.sets.remove(&semid);
            drop(state);
            crate::thread::with_thread_manager(|manager| manager.wake_io());
            Ok(0)
        }
        IPC_STAT | SEM_STAT => {
            if !has_access(set, process, false) {
                return Err(SyscallError::PermissionDenied);
            }
            if arg == 0 {
                return Err(SyscallError::BadAddress);
            }
            let ds = linux_semid_ds(set);
            drop(state);
            user_safe::write(arg as *mut LinuxSemidDs, &ds)?;
            Ok(if cmd == SEM_STAT { semid as usize } else { 0 })
        }
        IPC_SET => {
            if arg == 0 {
                return Err(SyscallError::BadAddress);
            }
            if process.effective_uid != 0
                && process.effective_uid != set.owner_uid
                && process.effective_uid != set.creator_uid
            {
                return Err(SyscallError::PermissionDenied);
            }
            let ds = user_safe::read(arg as *const LinuxSemidDs)?;
            set.owner_uid = ds.sem_perm.uid;
            set.owner_gid = ds.sem_perm.gid;
            set.mode = ds.sem_perm.mode & IPC_MODE_MASK as u32;
            set.ctime = now_seconds();
            Ok(0)
        }
        GETPID | GETVAL | GETNCNT | GETZCNT => {
            let index = usize::try_from(semnum).map_err(|_| SyscallError::InvalidArguments)?;
            if index >= set.values.len() {
                return Err(SyscallError::InvalidArguments);
            }
            if !has_access(set, process, false) {
                return Err(SyscallError::PermissionDenied);
            }
            Ok(match cmd {
                GETPID => set.last_pid as usize,
                GETVAL => set.values[index] as usize,
                GETNCNT | GETZCNT => 0,
                _ => unreachable!(),
            })
        }
        GETALL => {
            if !has_access(set, process, false) {
                return Err(SyscallError::PermissionDenied);
            }
            if arg == 0 {
                return Err(SyscallError::BadAddress);
            }
            for (index, value) in set.values.iter().copied().enumerate() {
                user_safe::write(unsafe { (arg as *mut u16).add(index) }, &value)?;
            }
            Ok(0)
        }
        SETVAL => {
            let index = usize::try_from(semnum).map_err(|_| SyscallError::InvalidArguments)?;
            if !has_access(set, process, true) {
                return Err(SyscallError::PermissionDenied);
            }
            if arg > SEMVMX as usize {
                return Err(SyscallError::RangeError);
            }
            let Some(value) = set.values.get_mut(index) else {
                return Err(SyscallError::InvalidArguments);
            };
            *value = arg as u16;
            set.ctime = now_seconds();
            drop(state);
            crate::thread::with_thread_manager(|manager| manager.wake_io());
            Ok(0)
        }
        SETALL => {
            if !has_access(set, process, true) {
                return Err(SyscallError::PermissionDenied);
            }
            if arg == 0 {
                return Err(SyscallError::BadAddress);
            }
            let mut values = Vec::with_capacity(set.values.len());
            for index in 0..set.values.len() {
                let value = user_safe::read(unsafe { (arg as *const u16).add(index) })?;
                if value > SEMVMX {
                    return Err(SyscallError::RangeError);
                }
                values.push(value);
            }
            set.values = values;
            set.ctime = now_seconds();
            drop(state);
            crate::thread::with_thread_manager(|manager| manager.wake_io());
            Ok(0)
        }
        _ => Err(SyscallError::InvalidArguments),
    }
}

pub fn semtimedop(
    process: &Process,
    semid: i32,
    ops: *const LinuxSembuf,
    nsops: usize,
    timeout: *const LinuxTimespec,
) -> SyscallResult {
    let ops = read_ops(ops, nsops)?;
    let deadline = timeout_deadline(timeout)?;
    let nowait = ops.iter().any(|op| op.sem_flg & IPC_NOWAIT != 0);

    loop {
        {
            let mut state = SYSV_SEM_STATE.lock();
            let set = state
                .sets
                .get_mut(&semid)
                .filter(|set| !set.removed)
                .ok_or(SyscallError::IdentifierRemoved)?;
            if !has_access(set, process, true) {
                return Err(SyscallError::PermissionDenied);
            }
            if ops_ready(set, &ops)? {
                apply_ops(set, &ops, process.pid.0 as i32)?;
                return Ok(0);
            }
        }

        if nowait {
            return Err(SyscallError::TryAgain);
        }
        if let Some(deadline) = deadline
            && Time::since_boot() >= deadline
        {
            return Err(SyscallError::TryAgain);
        }
        match block_current_with_sig_check(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline,
        }) {
            Ok(()) => {}
            Err(err) => {
                let err = SyscallError::from(err);
                if matches!(err, SyscallError::Interrupted)
                    && deadline.is_some_and(|deadline| Time::since_boot() >= deadline)
                {
                    return Err(SyscallError::TryAgain);
                }
                return Err(err);
            }
        }
    }
}

pub fn semop(
    process: &Process,
    semid: i32,
    ops: *const LinuxSembuf,
    nsops: usize,
) -> SyscallResult {
    semtimedop(process, semid, ops, nsops, core::ptr::null())
}

pub fn proc_sysvipc_sem_bytes() -> Vec<u8> {
    let state = SYSV_SEM_STATE.lock();
    let mut out =
        b"       key      semid perms      nsems   uid   gid  cuid  cgid      otime      ctime\n"
            .to_vec();
    for (semid, set) in &state.sets {
        out.extend_from_slice(
            alloc::format!(
                "{:10} {:10} {:5o} {:10} {:5} {:5} {:5} {:5} {:10} {:10}\n",
                set.key,
                semid,
                set.mode & IPC_MODE_MASK as u32,
                set.values.len(),
                set.owner_uid,
                set.owner_gid,
                set.creator_uid,
                set.creator_gid,
                set.otime,
                set.ctime
            )
            .as_bytes(),
        );
    }
    out
}
