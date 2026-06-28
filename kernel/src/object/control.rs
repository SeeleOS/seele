use alloc::{collections::BTreeMap, format, string::String, sync::Arc, vec::Vec};

use crate::{
    memory::{user_safe, utils::Mut},
    object::{
        FileFlags,
        file_locks::{LinuxFlock, fcntl_get_lock, fcntl_set_lock},
        memfd::{memfd_add_seals, memfd_get_seals},
        misc::{ObjectRef, get_object_current_process},
    },
    process::{
        FdEntry, FdFlags, ProcessRef,
        fd_table::FdTableRef,
        group::ProcessGroupID,
        manager::{MANAGER, get_current_process},
        misc::{ProcessID, get_process_with_pid, with_current_process},
    },
    signal::{
        SigInfo, Signal, send_signal_to_process_with_siginfo, send_signal_to_thread_with_siginfo,
    },
    systemcall::utils::{SyscallError, SyscallResult},
    thread::{misc::ThreadID, with_thread_manager},
};
use bitflags::bitflags;
use lazy_static::lazy_static;
use num_enum::TryFromPrimitive;

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum FcntlCmd {
    DupFd = 0,
    GetFd = 1,
    SetFd = 2,
    GetFl = 3,
    SetFl = 4,
    GetLk = 5,
    SetLk = 6,
    SetLkw = 7,
    SetOwn = 8,
    GetOwn = 9,
    SetSig = 10,
    GetSig = 11,
    SetOwnEx = 15,
    GetOwnEx = 16,
    OfdGetLk = 36,
    OfdSetLk = 37,
    OfdSetLkw = 38,
    SetLease = 1024,
    GetLease = 1025,
    DupFdCloexec = 1030,
    SetPipeSz = 1031,
    GetPipeSz = 1032,
    AddSeals = 1033,
    GetSeals = 1034,
    CreatedQuery = 1028,
}

const O_WRONLY: usize = 0o1;
const O_RDWR: usize = 0o2;
const F_RDLCK: i32 = 0;
const F_WRLCK: i32 = 1;
const F_UNLCK: i32 = 2;
const F_OWNER_TID: i32 = 0;
const F_OWNER_PID: i32 = 1;
const F_OWNER_PGRP: i32 = 2;
const POLL_IN: i32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct LinuxFOwnerEx {
    owner_type: i32,
    pid: i32,
}

#[derive(Clone, Copy, Debug)]
struct FcntlObjectState {
    owner: LinuxFOwnerEx,
    signal: i32,
    lease: i32,
}

impl Default for FcntlObjectState {
    fn default() -> Self {
        Self {
            owner: LinuxFOwnerEx::default(),
            signal: 0,
            lease: F_UNLCK,
        }
    }
}

lazy_static! {
    static ref FCNTL_OBJECT_STATE: Mut<BTreeMap<String, FcntlObjectState>> =
        Mut::new(BTreeMap::new());
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct FileStatusFlags: u64 {
        const O_APPEND = 0o2_000;
        const O_NONBLOCK = 0o4_000;
        const O_ASYNC = 0o20_000;
        const O_DIRECT = 0o40_000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct DescriptorFlags: u32 {
        const FD_CLOEXEC = 1;
    }
}

fn access_mode_bits(object: &ObjectRef) -> usize {
    match object.clone().get_flags() {
        Ok(flags) if flags.contains(FileFlags::RDWR) => O_RDWR,
        Ok(flags) if flags.contains(FileFlags::WRONLY) => O_WRONLY,
        Ok(_) => 0,
        _ => {
            let readable = object.clone().as_readable().is_ok();
            let writable = object.clone().as_writable().is_ok();

            match (readable, writable) {
                (false, true) => O_WRONLY,
                (true, true) => O_RDWR,
                _ => 0,
            }
        }
    }
}

fn fcntl_object_key(object: &ObjectRef) -> String {
    if let Ok(file_like) = object.clone().as_file_like() {
        return format!(
            "file-path:{}:{}",
            file_like.mount_id(),
            file_like.path().normalize().as_string()
        );
    }

    format!("object:{:p}", Arc::as_ptr(object))
}

fn fcntl_object_state(object: &ObjectRef) -> FcntlObjectState {
    FCNTL_OBJECT_STATE
        .lock()
        .get(&fcntl_object_key(object))
        .copied()
        .unwrap_or_default()
}

fn update_fcntl_object_state(object: &ObjectRef, update: impl FnOnce(&mut FcntlObjectState)) {
    let mut states = FCNTL_OBJECT_STATE.lock();
    let state = states.entry(fcntl_object_key(object)).or_default();
    update(state);
}

fn validate_owner_ex(owner: LinuxFOwnerEx) -> SyscallResult {
    match owner.owner_type {
        F_OWNER_TID | F_OWNER_PID | F_OWNER_PGRP => Ok(0),
        _ => Err(SyscallError::InvalidArguments),
    }
}

fn process_pid_visible_to_current(pid: i32) -> Option<ProcessID> {
    if pid <= 0 {
        return None;
    }
    let viewer_namespace_inode = get_current_process().lock().pid_namespace.inode();
    MANAGER.lock().processes.values().find_map(|process| {
        let process = process.lock();
        (process.pid_visible_from_namespace_inode(viewer_namespace_inode) == Some(pid as u64))
            .then_some(process.pid)
    })
}

fn thread_tid_visible_to_current(tid: i32) -> Option<ThreadID> {
    if tid <= 0 {
        return None;
    }
    let viewer_namespace_inode = get_current_process().lock().pid_namespace.inode();
    with_thread_manager(|manager| {
        manager.threads.values().find_map(|thread| {
            let thread = thread.lock();
            let process = thread.parent.lock();
            let visible_pid = process.pid_visible_from_namespace_inode(viewer_namespace_inode)?;
            (visible_pid == tid as u64).then_some(thread.id)
        })
    })
}

fn owner_from_legacy_pid(owner_pid: i32) -> LinuxFOwnerEx {
    if owner_pid < 0 {
        LinuxFOwnerEx {
            owner_type: F_OWNER_PGRP,
            pid: -owner_pid,
        }
    } else {
        LinuxFOwnerEx {
            owner_type: F_OWNER_PID,
            pid: process_pid_visible_to_current(owner_pid)
                .map(|pid| pid.0 as i32)
                .unwrap_or(owner_pid),
        }
    }
}

fn fd_entry_is_other_open_file(entry: &FdEntry, object: &ObjectRef) -> bool {
    if Arc::ptr_eq(&entry.object, object) {
        return false;
    }

    fcntl_object_key(&entry.object) == fcntl_object_key(object)
}

fn fd_table_has_other_open_file_description(
    fd_table: &[Option<FdEntry>],
    fd: Option<usize>,
    object: &ObjectRef,
) -> bool {
    fd_table
        .iter()
        .enumerate()
        .filter(|(entry_fd, _)| Some(*entry_fd) != fd)
        .filter_map(|(_, entry)| entry.as_ref())
        .any(|entry| fd_entry_is_other_open_file(entry, object))
}

fn release_fcntl_object_state_by_key(object_key: &str) {
    FCNTL_OBJECT_STATE.lock().remove(object_key);
}

fn release_fcntl_object_state_for_object(object: &ObjectRef) {
    let object_key = fcntl_object_key(object);
    release_fcntl_object_state_by_key(&object_key);
}

fn has_other_open_file_description(
    object: &ObjectRef,
    current_process: &ProcessRef,
    current_fd_table: &FdTableRef,
) -> bool {
    let processes = MANAGER
        .lock()
        .processes
        .values()
        .cloned()
        .collect::<Vec<_>>();

    processes.into_iter().any(|process| {
        if Arc::ptr_eq(&process, current_process) {
            return false;
        }
        let Some(process) = process.try_lock() else {
            return true;
        };
        let fd_table = process.fd_table.clone();
        if Arc::ptr_eq(&fd_table, current_fd_table) {
            return false;
        }
        let Some(fd_table) = fd_table.try_lock() else {
            return true;
        };
        fd_table_has_other_open_file_description(&fd_table, None, object)
    })
}

fn has_other_open_file_description_for_release(object: &ObjectRef) -> bool {
    let processes = MANAGER
        .lock()
        .processes
        .values()
        .cloned()
        .collect::<Vec<_>>();

    processes.into_iter().any(|process| {
        let Some(process) = process.try_lock() else {
            return true;
        };
        let Some(fd_table) = process.fd_table.try_lock() else {
            return true;
        };
        fd_table_has_other_open_file_description(&fd_table, None, object)
    })
}

fn fcntl_set_lease(
    object: &ObjectRef,
    lease: i32,
    current_process_has_other_open_file: bool,
    current_process: &ProcessRef,
    current_fd_table: &FdTableRef,
) -> SyscallResult {
    if !matches!(lease, F_RDLCK | F_WRLCK | F_UNLCK) {
        return Err(SyscallError::InvalidArguments);
    }
    if lease == F_UNLCK {
        update_fcntl_object_state(object, |state| state.lease = lease);
        return Ok(0);
    }
    if lease == F_RDLCK && matches!(access_mode_bits(object), O_WRONLY | O_RDWR) {
        return Err(SyscallError::TryAgain);
    }
    if lease == F_WRLCK
        && (current_process_has_other_open_file
            || has_other_open_file_description(object, current_process, current_fd_table))
    {
        return Err(SyscallError::TryAgain);
    }

    update_fcntl_object_state(object, |state| state.lease = lease);
    Ok(0)
}

pub(crate) fn notify_fcntl_async_readable(object: &ObjectRef) {
    let state = fcntl_object_state(object);
    if state.owner.pid == 0 {
        return;
    }
    let signal = if state.signal == 0 {
        Signal::SIGIO
    } else {
        match Signal::try_from(state.signal as u64) {
            Ok(signal) => signal,
            Err(_) => return,
        }
    };

    match state.owner.owner_type {
        F_OWNER_TID => {
            if let Some(thread) = with_thread_manager(|manager| {
                manager
                    .threads
                    .get(&ThreadID(state.owner.pid as u64))
                    .cloned()
            }) {
                let process = thread.lock().parent.clone();
                send_signal_to_thread_with_siginfo(
                    &thread,
                    signal,
                    SigInfo::for_poll_signal(
                        signal,
                        POLL_IN,
                        fd_for_object_owner(&process, object),
                    ),
                );
            }
        }
        F_OWNER_PID => {
            if let Ok(process) = get_process_with_pid(ProcessID(state.owner.pid as u64)) {
                send_signal_to_process_with_siginfo(
                    &process,
                    signal,
                    SigInfo::for_poll_signal(
                        signal,
                        POLL_IN,
                        fd_for_object_owner(&process, object),
                    ),
                );
            }
        }
        F_OWNER_PGRP => {
            let group_id = ProcessGroupID(state.owner.pid as u64);
            let processes = MANAGER
                .lock()
                .processes
                .values()
                .cloned()
                .collect::<Vec<_>>();
            for process in processes {
                if process.lock().group_id == group_id {
                    send_signal_to_process_with_siginfo(
                        &process,
                        signal,
                        SigInfo::for_poll_signal(
                            signal,
                            POLL_IN,
                            fd_for_object_owner(&process, object),
                        ),
                    );
                }
            }
        }
        _ => {}
    }
}

fn fd_for_object_owner(process: &crate::process::ProcessRef, object: &ObjectRef) -> i32 {
    let process = process.lock();
    process
        .fd_table
        .lock()
        .iter()
        .position(|entry| {
            entry
                .as_ref()
                .is_some_and(|entry| Arc::ptr_eq(&entry.object, object))
        })
        .map(|fd| fd as i32)
        .unwrap_or(-1)
}

pub(crate) fn release_fcntl_object_state(object: &ObjectRef) {
    if Arc::strong_count(object) == 1 && !has_other_open_file_description_for_release(object) {
        release_fcntl_object_state_for_object(object);
    }
}

pub fn control_object(fd: u64, command: u64, arg: u64) -> SyscallResult {
    let command = FcntlCmd::try_from(command).map_err(|_| SyscallError::InvalidArguments)?;
    match command {
        FcntlCmd::DupFd | FcntlCmd::DupFdCloexec => {
            with_current_process(|process| {
                if arg >= process.rlimit_nofile_cur {
                    return Err(SyscallError::TooManyOpenFilesProcess);
                }
                Ok(())
            })?;
        }
        _ => {}
    }
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    match command {
        FcntlCmd::SetFl => {
            let mut flags = object.clone().get_flags().map_err(SyscallError::from)?
                & (FileFlags::WRONLY | FileFlags::RDWR);
            let status_flags = FileStatusFlags::from_bits_truncate(arg);
            if status_flags.contains(FileStatusFlags::O_APPEND) {
                flags.insert(FileFlags::APPEND);
            }
            if status_flags.contains(FileStatusFlags::O_NONBLOCK) {
                flags.insert(FileFlags::NONBLOCK);
            }
            if status_flags.contains(FileStatusFlags::O_DIRECT) {
                flags.insert(FileFlags::DIRECT);
            }
            if status_flags.contains(FileStatusFlags::O_ASYNC) {
                flags.insert(FileFlags::ASYNC);
            }
            match object.set_flags(flags) {
                Ok(()) => Ok(0),
                Err(err) => Err(err.into()),
            }
        }
        FcntlCmd::GetFl => {
            let flags = match object.clone().get_flags() {
                Ok(flags) => {
                    let mut linux_flags = 0;
                    if flags.contains(FileFlags::APPEND) {
                        linux_flags |= FileStatusFlags::O_APPEND.bits() as usize;
                    }
                    if flags.contains(FileFlags::NONBLOCK) {
                        linux_flags |= FileStatusFlags::O_NONBLOCK.bits() as usize;
                    }
                    if flags.contains(FileFlags::DIRECT) {
                        linux_flags |= FileStatusFlags::O_DIRECT.bits() as usize;
                    }
                    if flags.contains(FileFlags::ASYNC) {
                        linux_flags |= FileStatusFlags::O_ASYNC.bits() as usize;
                    }
                    linux_flags
                }
                Err(err) => return Err(err.into()),
            };

            Ok(access_mode_bits(&object) | flags)
        }
        FcntlCmd::DupFd => with_current_process(|process| {
            process
                .clone_object_with_min(object, arg as usize)
                .map_err(Into::into)
        }),
        FcntlCmd::DupFdCloexec => with_current_process(|process| {
            process
                .clone_object_with_min_and_flags(object, arg as usize, FdFlags::CLOEXEC)
                .map_err(Into::into)
        }),
        FcntlCmd::GetFd => {
            with_current_process(|process| Ok(process.get_fd_flags(fd as usize)?.bits() as usize))
        }
        FcntlCmd::CreatedQuery => with_current_process(|process| {
            Ok(usize::from(process.fd_created_by_open(fd as usize)?))
        }),
        FcntlCmd::SetFd => with_current_process(|process| {
            let descriptor_flags = DescriptorFlags::from_bits_truncate(arg as u32);
            let flags = if descriptor_flags.contains(DescriptorFlags::FD_CLOEXEC) {
                FdFlags::CLOEXEC
            } else {
                FdFlags::empty()
            };
            process.set_fd_flags(fd as usize, flags)?;
            Ok(0)
        }),
        FcntlCmd::SetOwn => {
            let owner_pid = arg as i32;
            update_fcntl_object_state(&object, |state| {
                state.owner = owner_from_legacy_pid(owner_pid)
            });
            Ok(0)
        }
        FcntlCmd::GetOwn => {
            let owner = fcntl_object_state(&object).owner;
            Ok(match owner.owner_type {
                F_OWNER_PGRP => -(owner.pid as isize) as usize,
                _ => owner.pid as usize,
            })
        }
        FcntlCmd::SetOwnEx => {
            let mut owner = user_safe::read(arg as *const LinuxFOwnerEx)?;
            validate_owner_ex(owner)?;
            match owner.owner_type {
                F_OWNER_PID => {
                    if let Some(global_pid) = process_pid_visible_to_current(owner.pid) {
                        owner.pid = global_pid.0 as i32;
                    }
                }
                F_OWNER_TID => {
                    if let Some(thread_id) = thread_tid_visible_to_current(owner.pid) {
                        owner.pid = thread_id.0 as i32;
                    }
                }
                _ => {}
            }
            update_fcntl_object_state(&object, |state| state.owner = owner);
            Ok(0)
        }
        FcntlCmd::GetOwnEx => {
            user_safe::write(
                arg as *mut LinuxFOwnerEx,
                &fcntl_object_state(&object).owner,
            )?;
            Ok(0)
        }
        FcntlCmd::SetSig => {
            if arg != 0 {
                let _ = crate::signal::Signal::try_from(arg)
                    .map_err(|_| SyscallError::InvalidArguments)?;
            }
            update_fcntl_object_state(&object, |state| state.signal = arg as i32);
            Ok(0)
        }
        FcntlCmd::GetSig => Ok(fcntl_object_state(&object).signal as usize),
        FcntlCmd::GetLk | FcntlCmd::OfdGetLk => fcntl_get_lock(
            &object,
            arg as *mut LinuxFlock,
            matches!(command, FcntlCmd::OfdGetLk),
        ),
        FcntlCmd::SetLk | FcntlCmd::SetLkw | FcntlCmd::OfdSetLk | FcntlCmd::OfdSetLkw => {
            fcntl_set_lock(
                &object,
                arg as *mut LinuxFlock,
                matches!(command, FcntlCmd::OfdSetLk | FcntlCmd::OfdSetLkw),
                matches!(command, FcntlCmd::SetLkw | FcntlCmd::OfdSetLkw),
            )
        }
        FcntlCmd::SetPipeSz => {
            let pipe = object.as_pipe()?;
            Ok(pipe.set_capacity(arg as usize)?)
        }
        FcntlCmd::GetPipeSz => {
            let pipe = object.as_pipe()?;
            Ok(pipe.capacity())
        }
        FcntlCmd::SetLease => {
            let current_process = get_current_process();
            let Some(current_process_guard) = current_process.try_lock() else {
                return Err(SyscallError::TryAgain);
            };
            let current_fd_table = current_process_guard.fd_table.clone();
            let Some(current_fd_table_guard) = current_fd_table.try_lock() else {
                return Err(SyscallError::TryAgain);
            };
            let current_process_has_other_open_file = fd_table_has_other_open_file_description(
                &current_fd_table_guard,
                Some(fd as usize),
                &object,
            );
            fcntl_set_lease(
                &object,
                arg as i32,
                current_process_has_other_open_file,
                &current_process,
                &current_fd_table,
            )
        }
        FcntlCmd::GetLease => Ok(fcntl_object_state(&object).lease as usize),
        FcntlCmd::AddSeals => {
            let flags = object.clone().get_flags().map_err(SyscallError::from)?;
            if !flags.intersects(FileFlags::WRONLY | FileFlags::RDWR) {
                return Err(SyscallError::PermissionDenied);
            }
            let file_like = object.as_file_like()?;
            memfd_add_seals(&file_like.path(), arg as u32)
        }
        FcntlCmd::GetSeals => {
            let file_like = object.as_file_like()?;
            memfd_get_seals(&file_like.path())
                .map(|seals| seals as usize)
                .ok_or(SyscallError::InvalidArguments)
        }
    }
}
