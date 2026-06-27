use alloc::{collections::BTreeMap, vec::Vec};
use core::mem;

use lazy_static::lazy_static;

use crate::{
    memory::{user_safe, utils::Mut},
    misc::time::unix_timestamp_seconds,
    systemcall::utils::{SyscallError, SyscallResult},
    thread::yielding::{BlockType, WakeType, block_current_with_sig_check},
};

const IPC_PRIVATE: i32 = 0;
const IPC_CREAT: i32 = 0o1000;
const IPC_EXCL: i32 = 0o2000;
const IPC_NOWAIT: i32 = 0o4000;
const MSG_NOERROR: i32 = 0o10000;
const IPC_RMID: i32 = 0;
const IPC_SET: i32 = 1;
const IPC_STAT: i32 = 2;
const IPC_INFO: i32 = 3;
const MSG_STAT: i32 = 11;
const MSG_INFO: i32 = 12;
const IPC_MODE_MASK: i32 = 0o777;
const MSGMNI: usize = 32000;
const MSGMAX: usize = 8192;
const MSGMNB: usize = 16384;

lazy_static! {
    static ref SYSV_MSG_STATE: Mut<SysvMsgState> = Mut::new(SysvMsgState::default());
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
pub struct LinuxMsqidDs {
    pub msg_perm: LinuxIpcPerm,
    pub msg_stime: i64,
    pub msg_rtime: i64,
    pub msg_ctime: i64,
    pub __msg_cbytes: u64,
    pub msg_qnum: u64,
    pub msg_qbytes: u64,
    pub msg_lspid: i32,
    pub msg_lrpid: i32,
    pub __pad1: u64,
    pub __pad2: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxMsginfo {
    pub msgpool: i32,
    pub msgmap: i32,
    pub msgmax: i32,
    pub msgmnb: i32,
    pub msgmni: i32,
    pub msgssz: i32,
    pub msgtql: i32,
    pub msgseg: u16,
}

#[derive(Clone, Debug)]
pub struct SysvMsgCredentials {
    pub namespace_inode: u64,
    pub pid: i32,
    pub effective_uid: u32,
    pub effective_gid: u32,
    pub supplementary_groups: Vec<u32>,
}

#[derive(Debug, Default)]
struct SysvMsgState {
    next_msqid: i32,
    queues: BTreeMap<i32, SysvMsgQueue>,
}

#[derive(Clone, Debug)]
struct SysvMsgQueue {
    namespace_inode: u64,
    key: i32,
    messages: Vec<SysvMessage>,
    bytes: usize,
    qbytes: usize,
    last_send_pid: i32,
    last_recv_pid: i32,
    owner_uid: u32,
    owner_gid: u32,
    creator_uid: u32,
    creator_gid: u32,
    mode: u32,
    seq: i32,
    stime: i64,
    rtime: i64,
    ctime: i64,
    removed: bool,
}

#[derive(Clone, Debug)]
struct SysvMessage {
    ty: i64,
    data: Vec<u8>,
}

impl SysvMsgState {
    fn next_msqid(&mut self) -> i32 {
        self.next_msqid += 1;
        self.next_msqid
    }
}

fn now_seconds() -> i64 {
    unix_timestamp_seconds().min(i64::MAX as u64) as i64
}

fn msginfo() -> LinuxMsginfo {
    LinuxMsginfo {
        msgpool: MSGMNI as i32,
        msgmap: MSGMNI as i32,
        msgmax: MSGMAX as i32,
        msgmnb: MSGMNB as i32,
        msgmni: MSGMNI as i32,
        msgssz: 16,
        msgtql: MSGMNI as i32,
        msgseg: u16::MAX,
    }
}

fn has_access(queue: &SysvMsgQueue, credentials: &SysvMsgCredentials, write: bool) -> bool {
    if credentials.effective_uid == 0 {
        return true;
    }

    let mut mask = 0o4u32;
    if write {
        mask |= 0o2;
    }
    let mode = queue.mode;
    if credentials.effective_uid == queue.owner_uid
        || credentials.effective_uid == queue.creator_uid
    {
        return mode & (mask << 6) == (mask << 6);
    }
    if credentials.effective_gid == queue.owner_gid
        || credentials.effective_gid == queue.creator_gid
        || credentials
            .supplementary_groups
            .iter()
            .any(|gid| *gid == queue.owner_gid || *gid == queue.creator_gid)
    {
        return mode & (mask << 3) == (mask << 3);
    }
    mode & mask == mask
}

fn linux_msqid_ds(queue: &SysvMsgQueue) -> LinuxMsqidDs {
    LinuxMsqidDs {
        msg_perm: LinuxIpcPerm {
            __ipc_perm_key: queue.key,
            uid: queue.owner_uid,
            gid: queue.owner_gid,
            cuid: queue.creator_uid,
            cgid: queue.creator_gid,
            mode: queue.mode,
            __ipc_perm_seq: queue.seq,
            __pad1: 0,
            __pad2: 0,
        },
        msg_stime: queue.stime,
        msg_rtime: queue.rtime,
        msg_ctime: queue.ctime,
        __msg_cbytes: queue.bytes as u64,
        msg_qnum: queue.messages.len() as u64,
        msg_qbytes: queue.qbytes as u64,
        msg_lspid: queue.last_send_pid,
        msg_lrpid: queue.last_recv_pid,
        __pad1: 0,
        __pad2: 0,
    }
}

fn select_message(messages: &[SysvMessage], msgtyp: i64) -> Option<usize> {
    if msgtyp == 0 {
        return (!messages.is_empty()).then_some(0);
    }
    if msgtyp > 0 {
        return messages.iter().position(|message| message.ty == msgtyp);
    }
    let limit = -msgtyp;
    messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.ty <= limit)
        .min_by_key(|(_, message)| message.ty)
        .map(|(index, _)| index)
}

pub fn msgget(credentials: &SysvMsgCredentials, key: i32, msgflg: i32) -> SyscallResult {
    let create = msgflg & IPC_CREAT != 0;
    let exclusive = msgflg & IPC_EXCL != 0;

    let mut state = SYSV_MSG_STATE.lock();
    if key != IPC_PRIVATE
        && let Some((msqid, queue)) = state.queues.iter().find(|(_, queue)| {
            queue.namespace_inode == credentials.namespace_inode
                && queue.key == key
                && !queue.removed
        })
    {
        if create && exclusive {
            return Err(SyscallError::FileAlreadyExists);
        }
        if !has_access(queue, credentials, false) {
            return Err(SyscallError::PermissionDenied);
        }
        return Ok(*msqid as usize);
    }

    if !create && key != IPC_PRIVATE {
        return Err(SyscallError::FileNotFound);
    }
    if state.queues.len() >= MSGMNI {
        return Err(SyscallError::NoSpaceLeft);
    }

    let msqid = state.next_msqid();
    let now = now_seconds();
    state.queues.insert(
        msqid,
        SysvMsgQueue {
            namespace_inode: credentials.namespace_inode,
            key,
            messages: Vec::new(),
            bytes: 0,
            qbytes: MSGMNB,
            last_send_pid: 0,
            last_recv_pid: 0,
            owner_uid: credentials.effective_uid,
            owner_gid: credentials.effective_gid,
            creator_uid: credentials.effective_uid,
            creator_gid: credentials.effective_gid,
            mode: (msgflg & IPC_MODE_MASK) as u32,
            seq: 0,
            stime: 0,
            rtime: 0,
            ctime: now,
            removed: false,
        },
    );
    Ok(msqid as usize)
}

#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "syscall implementations take user pointers and access them through user_safe"
)]
pub fn msgsnd(
    credentials: &SysvMsgCredentials,
    msqid: i32,
    msgp: *const u8,
    msgsz: usize,
    msgflg: i32,
) -> SyscallResult {
    if msgp.is_null() {
        return Err(SyscallError::BadAddress);
    }
    if msgsz > MSGMAX {
        return Err(SyscallError::InvalidArguments);
    }
    let ty = user_safe::read(msgp as *const i64)?;
    if ty <= 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let data = user_safe::read_buffer(unsafe { msgp.add(mem::size_of::<i64>()) }, msgsz)?;

    loop {
        {
            let mut state = SYSV_MSG_STATE.lock();
            let queue = state
                .queues
                .get_mut(&msqid)
                .filter(|queue| {
                    queue.namespace_inode == credentials.namespace_inode && !queue.removed
                })
                .ok_or(SyscallError::InvalidArguments)?;
            if !has_access(queue, credentials, true) {
                return Err(SyscallError::PermissionDenied);
            }
            if queue.bytes + msgsz <= queue.qbytes {
                queue.bytes += msgsz;
                queue.messages.push(SysvMessage {
                    ty,
                    data: data.clone(),
                });
                queue.last_send_pid = credentials.pid;
                queue.stime = now_seconds();
                drop(state);
                crate::thread::with_thread_manager(|manager| manager.wake_io());
                return Ok(0);
            }
        }

        if msgflg & IPC_NOWAIT != 0 {
            return Err(SyscallError::TryAgain);
        }
        block_current_with_sig_check(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline: None,
        })
        .map_err(SyscallError::from)?;
    }
}

#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "syscall implementations take user pointers and access them through user_safe"
)]
pub fn msgrcv(
    credentials: &SysvMsgCredentials,
    msqid: i32,
    msgp: *mut u8,
    msgsz: usize,
    msgtyp: i64,
    msgflg: i32,
) -> SyscallResult {
    if msgp.is_null() {
        return Err(SyscallError::BadAddress);
    }

    loop {
        {
            let mut state = SYSV_MSG_STATE.lock();
            let queue = state
                .queues
                .get_mut(&msqid)
                .filter(|queue| {
                    queue.namespace_inode == credentials.namespace_inode && !queue.removed
                })
                .ok_or(SyscallError::IdentifierRemoved)?;
            if !has_access(queue, credentials, false) {
                return Err(SyscallError::PermissionDenied);
            }
            if let Some(index) = select_message(&queue.messages, msgtyp) {
                let message = queue.messages.remove(index);
                if message.data.len() > msgsz && msgflg & MSG_NOERROR == 0 {
                    queue.messages.insert(index, message);
                    return Err(SyscallError::InvalidArguments);
                }
                queue.bytes = queue.bytes.saturating_sub(message.data.len());
                queue.last_recv_pid = credentials.pid;
                queue.rtime = now_seconds();
                let copied = message.data.len().min(msgsz);
                let ty = message.ty;
                let data = message.data[..copied].to_vec();
                drop(state);
                user_safe::write(msgp as *mut i64, &ty)?;
                user_safe::write_buffer(unsafe { msgp.add(mem::size_of::<i64>()) }, &data)?;
                crate::thread::with_thread_manager(|manager| manager.wake_io());
                return Ok(copied);
            }
        }

        if msgflg & IPC_NOWAIT != 0 {
            return Err(SyscallError::NoMessage);
        }
        block_current_with_sig_check(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline: None,
        })
        .map_err(SyscallError::from)?;
    }
}

pub fn msgctl(
    credentials: &SysvMsgCredentials,
    msqid: i32,
    cmd: i32,
    buf: *mut LinuxMsqidDs,
) -> SyscallResult {
    match cmd {
        IPC_INFO | MSG_INFO => {
            if buf.is_null() {
                return Err(SyscallError::BadAddress);
            }
            user_safe::write(buf as *mut LinuxMsginfo, &msginfo())?;
            return Ok(MSGMNI - 1);
        }
        _ => {}
    }

    let mut state = SYSV_MSG_STATE.lock();
    let queue = state
        .queues
        .get_mut(&msqid)
        .filter(|queue| queue.namespace_inode == credentials.namespace_inode && !queue.removed)
        .ok_or(SyscallError::InvalidArguments)?;

    match cmd {
        IPC_RMID => {
            if credentials.effective_uid != 0
                && credentials.effective_uid != queue.owner_uid
                && credentials.effective_uid != queue.creator_uid
            {
                return Err(SyscallError::PermissionDenied);
            }
            queue.removed = true;
            state.queues.remove(&msqid);
            drop(state);
            crate::thread::with_thread_manager(|manager| manager.wake_io());
            Ok(0)
        }
        IPC_STAT | MSG_STAT => {
            if !has_access(queue, credentials, false) {
                return Err(SyscallError::PermissionDenied);
            }
            if buf.is_null() {
                return Err(SyscallError::BadAddress);
            }
            let ds = linux_msqid_ds(queue);
            drop(state);
            user_safe::write(buf, &ds)?;
            Ok(if cmd == MSG_STAT { msqid as usize } else { 0 })
        }
        IPC_SET => {
            if buf.is_null() {
                return Err(SyscallError::BadAddress);
            }
            if credentials.effective_uid != 0
                && credentials.effective_uid != queue.owner_uid
                && credentials.effective_uid != queue.creator_uid
            {
                return Err(SyscallError::PermissionDenied);
            }
            let ds = user_safe::read(buf)?;
            queue.owner_uid = ds.msg_perm.uid;
            queue.owner_gid = ds.msg_perm.gid;
            queue.mode = ds.msg_perm.mode & IPC_MODE_MASK as u32;
            queue.qbytes = ds.msg_qbytes.min(MSGMNB as u64) as usize;
            queue.ctime = now_seconds();
            Ok(0)
        }
        _ => Err(SyscallError::InvalidArguments),
    }
}

pub fn proc_sysvipc_msg_bytes() -> Vec<u8> {
    let namespace_inode = crate::process::manager::get_current_process()
        .lock()
        .ipc_namespace
        .inode();
    let state = SYSV_MSG_STATE.lock();
    let mut out = b"       key      msqid perms      cbytes       qnum lspid lrpid   uid   gid  cuid  cgid      stime      rtime      ctime\n".to_vec();
    for (msqid, queue) in &state.queues {
        if queue.namespace_inode != namespace_inode {
            continue;
        }
        out.extend_from_slice(
            alloc::format!(
                "{:10} {:10} {:5o} {:10} {:10} {:5} {:5} {:5} {:5} {:5} {:5} {:10} {:10} {:10}\n",
                queue.key,
                msqid,
                queue.mode & IPC_MODE_MASK as u32,
                queue.bytes,
                queue.messages.len(),
                queue.last_send_pid,
                queue.last_recv_pid,
                queue.owner_uid,
                queue.owner_gid,
                queue.creator_uid,
                queue.creator_gid,
                queue.stime,
                queue.rtime,
                queue.ctime
            )
            .as_bytes(),
        );
    }
    out
}
