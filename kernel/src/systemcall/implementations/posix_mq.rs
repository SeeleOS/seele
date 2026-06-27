use alloc::{collections::BTreeMap, string::String, sync::Arc};
use spin::Mutex;

use crate::{
    define_syscall,
    filesystem::info::LinuxStat,
    memory::{user_safe, utils::Mut},
    misc::signal::{SI_MESGQ, SigInfo},
    object::{FileFlags, Object, misc::ObjectRef, traits::Statable},
    process::{
        FdFlags, ProcessRef,
        manager::get_current_process,
        misc::{ProcessID, get_process_with_pid},
    },
    signal::{Signal, send_signal_to_process_with_siginfo},
    systemcall::utils::{SyscallError, SyscallImpl},
};

const O_ACCMODE: u32 = 0o3;
const O_CREAT: u32 = 0o100;
const O_EXCL: u32 = 0o200;
const O_NONBLOCK: u32 = 0o4000;
const O_CLOEXEC: u32 = 0o2000000;

static POSIX_MQUEUES: Mutex<BTreeMap<QueueKey, Arc<PosixMessageQueueObject>>> =
    Mutex::new(BTreeMap::new());

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct QueueKey {
    ipc_namespace_inode: u64,
    name: String,
}

#[derive(Debug)]
pub struct PosixMessageQueueObject {
    inode: u64,
    flags: Mut<FileFlags>,
    notification: Mut<Option<QueueNotification>>,
}

#[derive(Clone, Debug)]
struct QueueNotification {
    process_pid: ProcessID,
    signal: Signal,
    value: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxMqAttr {
    mq_flags: i64,
    mq_maxmsg: i64,
    mq_msgsize: i64,
    mq_curmsgs: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxSigevent {
    sigev_value: u64,
    sigev_signo: i32,
    sigev_notify: i32,
}

impl PosixMessageQueueObject {
    fn new(inode: u64, flags: FileFlags) -> Arc<Self> {
        Arc::new(Self {
            inode,
            flags: Mut::new(flags),
            notification: Mut::new(None),
        })
    }

    fn register_notification(
        &self,
        process: &ProcessRef,
        event: LinuxSigevent,
    ) -> Result<(), SyscallError> {
        const SIGEV_SIGNAL: i32 = 0;
        const SIGEV_NONE: i32 = 1;

        let notification = match event.sigev_notify {
            SIGEV_SIGNAL => Some(QueueNotification {
                process_pid: process.lock().pid,
                signal: Signal::try_from(event.sigev_signo as u64)
                    .map_err(|_| SyscallError::InvalidArguments)?,
                value: event.sigev_value,
            }),
            SIGEV_NONE => None,
            _ => return Err(SyscallError::InvalidArguments),
        };

        let mut current = self.notification.lock();
        if current.is_some() && notification.is_some() {
            return Err(SyscallError::DeviceOrResourceBusy);
        }
        *current = notification;
        Ok(())
    }

    fn notify_sender(&self, sender: &ProcessRef) {
        let Some(notification) = self.notification.lock().take() else {
            return;
        };
        let Ok(target) = get_process_with_pid(notification.process_pid) else {
            return;
        };
        let target_namespace_inode = target.lock().pid_namespace.inode();
        let (visible_sender_pid, sender_uid) = {
            let sender = sender.lock();
            let visible_pid = sender
                .pid_visible_from_namespace_inode(target_namespace_inode)
                .unwrap_or_else(|| {
                    if sender.pid_namespace_parent_inode == Some(target_namespace_inode) {
                        sender.pid.0
                    } else {
                        0
                    }
                });
            (visible_pid, sender.real_uid)
        };
        let mut siginfo =
            SigInfo::for_process_signal(notification.signal, visible_sender_pid as i32, sender_uid);
        siginfo.si_code = SI_MESGQ;
        siginfo.set_signal_value(notification.value);
        send_signal_to_process_with_siginfo(&target, notification.signal, siginfo);
    }

    fn attr(&self) -> LinuxMqAttr {
        LinuxMqAttr {
            mq_flags: if self.flags.lock().contains(FileFlags::NONBLOCK) {
                O_NONBLOCK as i64
            } else {
                0
            },
            mq_maxmsg: 10,
            mq_msgsize: 8192,
            mq_curmsgs: 0,
        }
    }
}

impl Object for PosixMessageQueueObject {
    fn get_flags(self: Arc<Self>) -> Result<FileFlags, crate::object::error::ObjectError> {
        Ok(*self.flags.lock())
    }

    fn set_flags(
        self: Arc<Self>,
        flags: FileFlags,
    ) -> Result<(), crate::object::error::ObjectError> {
        *self.flags.lock() = flags;
        Ok(())
    }

    fn as_statable(self: Arc<Self>) -> Result<Arc<dyn Statable>, SyscallError> {
        Ok(self)
    }

    fn as_posix_message_queue(
        self: Arc<Self>,
    ) -> Result<Arc<PosixMessageQueueObject>, SyscallError> {
        Ok(self)
    }
}

impl Statable for PosixMessageQueueObject {
    fn stat(&self) -> LinuxStat {
        const S_IFREG: u32 = 0o100000;

        LinuxStat {
            st_dev: 1,
            st_ino: self.inode,
            st_nlink: 1,
            st_mode: S_IFREG | 0o600,
            st_blksize: 4096,
            ..Default::default()
        }
    }
}

define_syscall!(MqOpen, |name: String, flags: u32, mode: u32, _attr: u64| {
    let _ = mode;
    let key = current_queue_key(name)?;
    let file_flags = file_flags_from_open_flags(flags)?;
    let fd_flags = if flags & O_CLOEXEC != 0 {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };

    let queue = {
        let mut queues = POSIX_MQUEUES.lock();
        if let Some(queue) = queues.get(&key) {
            if flags & O_CREAT != 0 && flags & O_EXCL != 0 {
                return Err(SyscallError::FileAlreadyExists);
            }
            queue.clone()
        } else {
            if flags & O_CREAT == 0 {
                return Err(SyscallError::FileNotFound);
            }
            let queue = PosixMessageQueueObject::new(next_queue_inode(), file_flags);
            queues.insert(key, queue.clone());
            queue
        }
    };

    let object: ObjectRef = queue;
    object.clone().set_flags(file_flags)?;
    Ok(get_current_process()
        .lock()
        .push_object_with_flags(object, fd_flags))
});

define_syscall!(MqUnlink, |name: String| {
    let key = current_queue_key(name)?;
    if POSIX_MQUEUES.lock().remove(&key).is_none() {
        return Err(SyscallError::FileNotFound);
    }
    Ok(0)
});

define_syscall!(MqTimedsend, |queue: ObjectRef,
                              _msg_ptr: *const u8,
                              msg_len: usize,
                              _priority: u32,
                              _timeout: u64| {
    if msg_len > 8192 {
        return Err(SyscallError::MessageTooLong);
    }
    let queue = queue.as_posix_message_queue()?;
    queue.notify_sender(&get_current_process());
    Ok(0)
});

define_syscall!(
    MqTimedreceive,
    |_queue: ObjectRef, _msg_ptr: *mut u8, _msg_len: usize, _priority: *mut u32, _timeout: u64| {
        Err(SyscallError::TryAgain)
    }
);

define_syscall!(MqNotify, |queue: ObjectRef, event: *const LinuxSigevent| {
    let queue = queue.as_posix_message_queue()?;
    if event.is_null() {
        *queue.notification.lock() = None;
        return Ok(0);
    }

    queue.register_notification(&get_current_process(), user_safe::read(event)?)?;
    Ok(0)
});

define_syscall!(
    MqGetsetattr,
    |queue: ObjectRef, new_attr: *const LinuxMqAttr, old_attr: *mut LinuxMqAttr| {
        let queue = queue.as_posix_message_queue()?;
        if !old_attr.is_null() {
            user_safe::write(old_attr, &queue.attr())?;
        }
        if !new_attr.is_null() {
            let attr = user_safe::read(new_attr)?;
            let mut flags = *queue.flags.lock();
            if attr.mq_flags & O_NONBLOCK as i64 != 0 {
                flags |= FileFlags::NONBLOCK;
            } else {
                flags.remove(FileFlags::NONBLOCK);
            }
            *queue.flags.lock() = flags;
        }
        Ok(0)
    }
);

fn current_queue_key(name: String) -> Result<QueueKey, SyscallError> {
    let name = normalize_queue_name(name)?;
    let ipc_namespace_inode = get_current_process().lock().ipc_namespace.inode();
    Ok(QueueKey {
        ipc_namespace_inode,
        name,
    })
}

fn normalize_queue_name(name: String) -> Result<String, SyscallError> {
    let name = name.strip_prefix('/').unwrap_or(&name);
    if name.is_empty() || name.contains('/') {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(String::from(name))
}

fn file_flags_from_open_flags(flags: u32) -> Result<FileFlags, SyscallError> {
    let access_mode = flags & O_ACCMODE;
    if access_mode == O_ACCMODE {
        return Err(SyscallError::InvalidArguments);
    }
    let mut file_flags = match access_mode {
        1 => FileFlags::WRONLY,
        2 => FileFlags::RDWR,
        _ => FileFlags::empty(),
    };
    if flags & O_NONBLOCK != 0 {
        file_flags |= FileFlags::NONBLOCK;
    }
    Ok(file_flags)
}

fn next_queue_inode() -> u64 {
    static NEXT_INODE: spin::Mutex<u64> = spin::Mutex::new(0x5000_0000);
    let mut next = NEXT_INODE.lock();
    let inode = *next;
    *next += 1;
    inode
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemcall::{
        implementations::{Close, MqGetsetattr, MqNotify, MqOpen, MqTimedreceive, MqTimedsend},
        test::{assert_fd_flags, assert_object_flags},
        test_helpers::{SyscallArgs, allocate_user_test_page, expect_errno, expect_ok},
        utils::SyscallError,
    };

    crate::test!(
        posix_message_queue_syscalls,
        "posix message queue syscalls follow linux rules",
        posix_message_queue_syscalls_follow_linux_rules
    );

    fn posix_message_queue_syscalls_follow_linux_rules() {
        let page = allocate_user_test_page();
        let queue_name = String::from("/mq-unit-test");
        let empty_name = String::new();

        expect_errno(
            SyscallArgs::new([&empty_name as *const String as u64, 0, 0, 0, 0, 0]).call::<MqOpen>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([&queue_name as *const String as u64, 0, 0, 0, 0, 0]).call::<MqOpen>(),
            SyscallError::FileNotFound,
        );

        let fd = SyscallArgs::new([
            &queue_name as *const String as u64,
            (O_CREAT | O_CLOEXEC | O_NONBLOCK) as u64,
            0o600,
            0,
            0,
            0,
        ])
        .call::<MqOpen>()
        .expect("mq_open should create a descriptor");
        assert_fd_flags(fd, FdFlags::CLOEXEC);
        assert_object_flags(fd, FileFlags::NONBLOCK);

        expect_errno(
            SyscallArgs::new([
                &queue_name as *const String as u64,
                (O_CREAT | O_EXCL) as u64,
                0o600,
                0,
                0,
                0,
            ])
            .call::<MqOpen>(),
            SyscallError::FileAlreadyExists,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, 0, page, 0, 0, 0]).call::<MqGetsetattr>(),
            0,
        );
        let old_attr = crate::memory::user_safe::read(page as *const LinuxMqAttr).unwrap();
        assert_eq!(old_attr.mq_flags, O_NONBLOCK as i64);
        assert_eq!(old_attr.mq_maxmsg, 10);
        assert_eq!(old_attr.mq_msgsize, 8192);

        crate::memory::user_safe::write(
            (page + 64) as *mut LinuxMqAttr,
            &LinuxMqAttr {
                mq_flags: 0,
                ..LinuxMqAttr::default()
            },
        )
        .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, page + 64, 0, 0, 0, 0]).call::<MqGetsetattr>(),
            0,
        );
        assert_object_flags(fd, FileFlags::empty());

        expect_errno(
            SyscallArgs::new([fd as u64, page + 128, 8193, 0, 0, 0]).call::<MqTimedsend>(),
            SyscallError::MessageTooLong,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, page + 128, 1, 0, 0, 0]).call::<MqTimedsend>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, page + 128, 1, 0, 0, 0]).call::<MqTimedreceive>(),
            SyscallError::TryAgain,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<MqNotify>(),
            0,
        );
        crate::memory::user_safe::write(
            page as *mut LinuxSigevent,
            &LinuxSigevent {
                sigev_value: 0x55,
                sigev_signo: Signal::SIGUSR1 as i32,
                sigev_notify: 0,
            },
        )
        .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, page, 0, 0, 0, 0]).call::<MqNotify>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, page, 0, 0, 0, 0]).call::<MqNotify>(),
            SyscallError::DeviceOrResourceBusy,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([&queue_name as *const String as u64, 0, 0, 0, 0, 0])
                .call::<MqUnlink>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([&queue_name as *const String as u64, 0, 0, 0, 0, 0])
                .call::<MqUnlink>(),
            SyscallError::FileNotFound,
        );
    }
}
