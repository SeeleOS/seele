use alloc::sync::{Arc, Weak};

use bitflags::bitflags;
use strum::IntoEnumIterator;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    memory::utils::Mut,
    object::{
        FileFlags, Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        traits::{Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::manager::MANAGER,
    signal::{PendingSignalInfo, Signal, Signals},
    thread::{
        manager::ThreadManager,
        yielding::{
            BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
            wake_pollers_for_object,
        },
    },
};

use super::registry::WatcherRegistry;

lazy_static::lazy_static! {
    static ref SIGNALFD_REGISTRY: Mut<WatcherRegistry<SignalfdObject>> = Mut::new(WatcherRegistry::default());
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct SignalfdFlags: i32 {
        const SFD_NONBLOCK = 0o4_000;
        const SFD_CLOEXEC = 0o2_000_000;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxSignalfdSiginfo {
    ssi_signo: u32,
    ssi_errno: i32,
    ssi_code: i32,
    ssi_pid: u32,
    ssi_uid: u32,
    ssi_fd: i32,
    ssi_tid: u32,
    ssi_band: u32,
    ssi_overrun: u32,
    ssi_trapno: u32,
    ssi_status: i32,
    ssi_int: i32,
    ssi_ptr: u64,
    ssi_utime: u64,
    ssi_stime: u64,
    ssi_addr: u64,
    ssi_addr_lsb: u16,
    __pad2: u16,
    ssi_syscall: i32,
    ssi_call_addr: u64,
    ssi_arch: u32,
    __pad: [u8; 28],
}

#[derive(Debug)]
pub struct SignalfdObject {
    flags: Mut<FileFlags>,
    mask: Mut<u64>,
    owner_pid: u64,
    self_ref: Mut<Option<Weak<SignalfdObject>>>,
}

impl SignalfdObject {
    pub fn new(owner_pid: u64, mask: u64, flags: SignalfdFlags) -> Arc<Self> {
        let signalfd = Arc::new(Self {
            flags: Mut::new(FileFlags::empty()),
            mask: Mut::new(mask),
            owner_pid,
            self_ref: Mut::new(None),
        });
        *signalfd.self_ref.lock() = Some(Arc::downgrade(&signalfd));
        if flags.contains(SignalfdFlags::SFD_NONBLOCK) {
            let _ = signalfd.clone().set_flags(FileFlags::NONBLOCK);
        }
        register_signalfd(owner_pid, &signalfd);
        signalfd
    }

    pub fn set_mask(&self, mask: u64) {
        *self.mask.lock() = mask;
    }

    fn self_object(&self) -> Option<ObjectRef> {
        self.self_ref
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(|object| object as ObjectRef)
    }

    fn owner_pending_signals(&self) -> Signals {
        MANAGER
            .lock()
            .processes
            .values()
            .find_map(|process| {
                let process = process.lock();
                (process.pid.0 == self.owner_pid).then_some(process.pending_signals)
            })
            .unwrap_or_default()
    }

    fn next_ready_signal(&self) -> Option<Signal> {
        let ready_mask = self.owner_pending_signals().bits() & *self.mask.lock();
        Signal::iter().find(|signal| (ready_mask & Signals::from(*signal).bits()) != 0)
    }

    fn take_next_signal(&self) -> Option<PendingSignalInfo> {
        let manager = MANAGER.lock();
        let process = manager
            .processes
            .values()
            .find(|process| process.lock().pid.0 == self.owner_pid)?
            .clone();
        let mut process = process.lock();
        let ready_mask = process.pending_signals.bits() & *self.mask.lock();
        let signal =
            Signal::iter().find(|signal| (ready_mask & Signals::from(*signal).bits()) != 0)?;
        process.pending_signals.remove(Signals::from(signal));
        Some(
            process.pending_signal_info[signal.index()]
                .take()
                .unwrap_or_else(|| PendingSignalInfo::for_signal(signal)),
        )
    }

    fn wake_waiters(&self) {
        crate::thread::with_thread_manager(|manager| {
            manager.wake_io();
        });
        if let Some(object) = self.self_object() {
            wake_pollers_for_object(object, PollableEvent::CanBeRead);
        }
    }
}

fn register_signalfd(pid: u64, signalfd: &Arc<SignalfdObject>) {
    SIGNALFD_REGISTRY.lock().register(pid, signalfd);
}

pub fn wake_signalfd_for_process(pid: u64) {
    let watchers = SIGNALFD_REGISTRY.lock().live_watchers(pid);

    for signalfd in watchers {
        if signalfd.next_ready_signal().is_some() {
            signalfd.wake_waiters();
        }
    }
}

pub fn wake_signalfd_for_process_with_manager(pid: u64, manager: &mut ThreadManager) {
    let watchers = SIGNALFD_REGISTRY.lock().live_watchers(pid);

    for _ in watchers {
        manager.wake_io();
    }
}

impl Object for SignalfdObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("readable", Readable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("signalfd", SignalfdObject);
}

impl Pollable for SignalfdObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeRead) && self.next_ready_signal().is_some()
    }
}

impl Readable for SignalfdObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        if buffer.len() < core::mem::size_of::<LinuxSignalfdSiginfo>() {
            return Err(ObjectError::InvalidArguments);
        }

        loop {
            if let Some(siginfo) = self.take_next_signal() {
                let info = LinuxSignalfdSiginfo {
                    ssi_signo: siginfo.si_signo as u32,
                    ssi_errno: siginfo.si_errno,
                    ssi_code: siginfo.si_code,
                    ssi_pid: siginfo.si_pid as u32,
                    ssi_uid: siginfo.si_uid,
                    ssi_int: siginfo.si_value as i32,
                    ssi_ptr: siginfo.si_value,
                    ..Default::default()
                };
                let raw = unsafe {
                    core::slice::from_raw_parts(
                        (&info as *const LinuxSignalfdSiginfo).cast::<u8>(),
                        core::mem::size_of::<LinuxSignalfdSiginfo>(),
                    )
                };
                buffer[..raw.len()].copy_from_slice(raw);
                return Ok(raw.len());
            }

            if self.flags.lock().contains(FileFlags::NONBLOCK) {
                return Err(ObjectError::TryAgain);
            }

            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });

            if self.next_ready_signal().is_some() {
                cancel_block(&current);
                continue;
            }

            finish_block_current();
        }
    }
}

impl Statable for SignalfdObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
    }
}
