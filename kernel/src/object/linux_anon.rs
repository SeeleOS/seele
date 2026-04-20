use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    misc::time::Time,
    object::{
        FileFlags, Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        traits::{Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::manager::MANAGER,
    process::misc::ProcessID,
    signal::{Signal, Signals},
    thread::{
        THREAD_MANAGER,
        manager::ThreadManager,
        yielding::{
            BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        },
    },
};
use strum::IntoEnumIterator;

const EVENTFD_SEMAPHORE: i32 = 0x1;
const EVENTFD_COUNTER_MAX: u64 = u64::MAX - 1;

#[derive(Default)]
struct SignalfdRegistry {
    watchers: BTreeMap<u64, Vec<Weak<SignalfdObject>>>,
}

#[derive(Default)]
struct PidFdRegistry {
    watchers: BTreeMap<u64, Vec<Weak<PidFdObject>>>,
}

lazy_static::lazy_static! {
    static ref SIGNALFD_REGISTRY: Mutex<SignalfdRegistry> = Mutex::new(SignalfdRegistry::default());
    static ref PIDFD_REGISTRY: Mutex<PidFdRegistry> = Mutex::new(PidFdRegistry::default());
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
pub struct PidFdObject {
    flags: Mutex<FileFlags>,
    pid: u64,
    self_ref: Mutex<Option<Weak<PidFdObject>>>,
}

impl PidFdObject {
    pub fn new(pid: u64) -> Arc<Self> {
        let pidfd = Arc::new(Self {
            flags: Mutex::new(FileFlags::empty()),
            pid,
            self_ref: Mutex::new(None),
        });
        *pidfd.self_ref.lock() = Some(Arc::downgrade(&pidfd));
        register_pidfd(pid, &pidfd);
        pidfd
    }

    pub fn pid(&self) -> u64 {
        self.pid
    }

    fn self_object(&self) -> Option<ObjectRef> {
        self.self_ref
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(|object| object as ObjectRef)
    }

    fn is_alive(&self) -> bool {
        MANAGER
            .lock()
            .processes
            .get(&ProcessID(self.pid))
            .is_some_and(|process| !process.lock().have_exited())
    }

    fn wake_waiters_with_manager(&self, manager: &mut ThreadManager) {
        manager.wake_io();
        if let Some(object) = self.self_object() {
            manager.wake_poller(object, PollableEvent::CanBeRead);
        }
    }
}

fn register_pidfd(pid: u64, pidfd: &Arc<PidFdObject>) {
    let mut registry = PIDFD_REGISTRY.lock();
    let watchers = registry.watchers.entry(pid).or_default();
    watchers.retain(|watcher| watcher.strong_count() > 0);
    watchers.push(Arc::downgrade(pidfd));
}

fn pidfds_for_process(pid: u64) -> Vec<Arc<PidFdObject>> {
    {
        let mut registry = PIDFD_REGISTRY.lock();
        let Some(watchers) = registry.watchers.get_mut(&pid) else {
            return Vec::new();
        };

        let mut strong = Vec::new();
        watchers.retain(|watcher| {
            if let Some(pidfd) = watcher.upgrade() {
                strong.push(pidfd);
                true
            } else {
                false
            }
        });
        strong
    }
}

pub fn wake_pidfd_for_process_with_manager(pid: u64, manager: &mut ThreadManager) {
    for pidfd in pidfds_for_process(pid) {
        pidfd.wake_waiters_with_manager(manager);
    }
}

pub fn wake_pidfd_for_process(pid: u64) {
    let watchers = pidfds_for_process(pid);
    if watchers.is_empty() {
        return;
    }

    let mut manager = THREAD_MANAGER.get().unwrap().lock();
    for pidfd in watchers {
        pidfd.wake_waiters_with_manager(&mut manager);
    }
}

impl Object for PidFdObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("pidfd", PidFdObject);
}

impl Pollable for PidFdObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeRead) && !self.is_alive()
    }
}

impl Statable for PidFdObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
    }
}

#[derive(Debug)]
pub struct SignalfdObject {
    flags: Mutex<FileFlags>,
    mask: Mutex<u64>,
    owner_pid: u64,
    self_ref: Mutex<Option<Weak<SignalfdObject>>>,
}

impl SignalfdObject {
    pub fn new(owner_pid: u64, mask: u64, flags: i32) -> Arc<Self> {
        let signalfd = Arc::new(Self {
            flags: Mutex::new(FileFlags::empty()),
            mask: Mutex::new(mask),
            owner_pid,
            self_ref: Mutex::new(None),
        });
        *signalfd.self_ref.lock() = Some(Arc::downgrade(&signalfd));
        if (flags & 0o4_000) != 0 {
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

    fn take_next_signal(&self) -> Option<Signal> {
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
        Some(signal)
    }

    fn wake_waiters(&self) {
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        manager.wake_io();
        if let Some(object) = self.self_object() {
            manager.wake_poller(object, PollableEvent::CanBeRead);
        }
    }
}

fn register_signalfd(pid: u64, signalfd: &Arc<SignalfdObject>) {
    let mut registry = SIGNALFD_REGISTRY.lock();
    let watchers = registry.watchers.entry(pid).or_default();
    watchers.retain(|watcher| watcher.strong_count() > 0);
    watchers.push(Arc::downgrade(signalfd));
}

pub fn wake_signalfd_for_process(pid: u64) {
    let watchers = {
        let mut registry = SIGNALFD_REGISTRY.lock();
        let Some(watchers) = registry.watchers.get_mut(&pid) else {
            return;
        };

        let mut strong = Vec::new();
        watchers.retain(|watcher| {
            if let Some(signalfd) = watcher.upgrade() {
                strong.push(signalfd);
                true
            } else {
                false
            }
        });
        strong
    };

    for signalfd in watchers {
        if signalfd.next_ready_signal().is_some() {
            signalfd.wake_waiters();
        }
    }
}

pub fn wake_signalfd_for_process_with_manager(pid: u64, manager: &mut ThreadManager) {
    let watchers = {
        let mut registry = SIGNALFD_REGISTRY.lock();
        let Some(watchers) = registry.watchers.get_mut(&pid) else {
            return;
        };

        let mut strong = Vec::new();
        watchers.retain(|watcher| {
            if let Some(signalfd) = watcher.upgrade() {
                strong.push(signalfd);
                true
            } else {
                false
            }
        });
        strong
    };

    for signalfd in watchers {
        if signalfd.next_ready_signal().is_some() {
            manager.wake_io();
            if let Some(object) = signalfd.self_object() {
                manager.wake_poller(object, PollableEvent::CanBeRead);
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct InotifyObject {
    flags: Mutex<FileFlags>,
    next_watch: Mutex<i32>,
}

impl InotifyObject {
    pub fn add_watch(&self) -> i32 {
        let mut next_watch = self.next_watch.lock();
        *next_watch += 1;
        *next_watch
    }
}

impl Object for InotifyObject {
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
    impl_cast_function_non_trait!("inotify", InotifyObject);
}

impl Pollable for InotifyObject {
    fn is_event_ready(&self, _event: PollableEvent) -> bool {
        false
    }
}

impl Readable for InotifyObject {
    fn read(&self, _buffer: &mut [u8]) -> ObjectResult<usize> {
        if self.flags.lock().contains(FileFlags::NONBLOCK) {
            return Err(ObjectError::TryAgain);
        }

        loop {
            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });

            if self.flags.lock().contains(FileFlags::NONBLOCK) {
                cancel_block(&current);
                return Err(ObjectError::TryAgain);
            }

            finish_block_current();
        }
    }
}

impl Statable for InotifyObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
    }
}

#[derive(Debug)]
struct EventFdState {
    counter: u64,
}

#[derive(Debug)]
pub struct EventFdObject {
    flags: Mutex<FileFlags>,
    state: Mutex<EventFdState>,
    semaphore: bool,
    self_ref: Mutex<Option<Weak<EventFdObject>>>,
}

impl EventFdObject {
    pub fn new(initial: u64, flags: i32) -> Arc<Self> {
        let eventfd = Arc::new(Self {
            flags: Mutex::new(FileFlags::empty()),
            state: Mutex::new(EventFdState { counter: initial }),
            semaphore: (flags & EVENTFD_SEMAPHORE) != 0,
            self_ref: Mutex::new(None),
        });
        *eventfd.self_ref.lock() = Some(Arc::downgrade(&eventfd));
        if (flags & 0o4_000) != 0 {
            let _ = eventfd.clone().set_flags(FileFlags::NONBLOCK);
        }
        eventfd
    }

    fn self_object(&self) -> Option<ObjectRef> {
        self.self_ref
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(|object| object as ObjectRef)
    }

    fn is_read_ready(&self) -> bool {
        self.state.lock().counter > 0
    }

    fn is_write_ready(&self) -> bool {
        self.state.lock().counter < EVENTFD_COUNTER_MAX
    }

    fn wake_waiters(&self, event: PollableEvent) {
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        manager.wake_io();
        if let Some(object) = self.self_object() {
            manager.wake_poller(object, event);
        }
    }
}

impl Object for EventFdObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("readable", Readable);
    impl_cast_function!("writable", Writable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("eventfd", EventFdObject);
}

impl Pollable for EventFdObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => self.is_read_ready(),
            PollableEvent::CanBeWritten => self.is_write_ready(),
            _ => false,
        }
    }
}

impl Readable for EventFdObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        if buffer.len() < core::mem::size_of::<u64>() {
            return Err(ObjectError::InvalidArguments);
        }

        loop {
            let value = {
                let mut state = self.state.lock();
                if state.counter == 0 {
                    None
                } else if self.semaphore {
                    state.counter -= 1;
                    Some(1u64)
                } else {
                    let value = state.counter;
                    state.counter = 0;
                    Some(value)
                }
            };

            if let Some(value) = value {
                buffer[..8].copy_from_slice(&value.to_ne_bytes());
                self.wake_waiters(PollableEvent::CanBeWritten);
                return Ok(8);
            }

            if self.flags.lock().contains(FileFlags::NONBLOCK) {
                return Err(ObjectError::TryAgain);
            }

            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });

            if self.is_read_ready() {
                cancel_block(&current);
                continue;
            }

            finish_block_current();
        }
    }
}

impl Writable for EventFdObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        if buffer.len() < core::mem::size_of::<u64>() {
            return Err(ObjectError::InvalidArguments);
        }

        let value = u64::from_ne_bytes(buffer[..8].try_into().unwrap());
        if value == u64::MAX {
            return Err(ObjectError::InvalidArguments);
        }

        loop {
            let wrote = {
                let mut state = self.state.lock();
                if value <= EVENTFD_COUNTER_MAX.saturating_sub(state.counter) {
                    state.counter += value;
                    true
                } else {
                    false
                }
            };

            if wrote {
                self.wake_waiters(PollableEvent::CanBeRead);
                return Ok(8);
            }

            if self.flags.lock().contains(FileFlags::NONBLOCK) {
                return Err(ObjectError::TryAgain);
            }

            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });

            if self.is_write_ready() {
                cancel_block(&current);
                continue;
            }

            finish_block_current();
        }
    }
}

impl Statable for EventFdObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
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
            if let Some(signal) = self.take_next_signal() {
                let info = LinuxSignalfdSiginfo {
                    ssi_signo: signal as u32,
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

#[derive(Debug, Clone, Copy, Default)]
struct TimerFdState {
    deadline: Option<Time>,
    interval_ns: u64,
    expirations: u64,
}

#[derive(Debug, Default)]
pub struct TimerFdObject {
    flags: Mutex<FileFlags>,
    state: Mutex<TimerFdState>,
}

impl TimerFdObject {
    pub fn set_timer(&self, deadline: Option<Time>, interval_ns: u64) {
        let mut state = self.state.lock();
        state.deadline = deadline;
        state.interval_ns = interval_ns;
        state.expirations = 0;
    }

    pub fn current_timer(&self) -> (Option<Time>, u64) {
        let state = self.state.lock();
        (state.deadline, state.interval_ns)
    }

    fn refresh(state: &mut TimerFdState) {
        let Some(mut deadline) = state.deadline else {
            return;
        };

        let now = Time::since_boot();
        if deadline > now {
            return;
        }

        if state.interval_ns == 0 {
            state.expirations = state.expirations.saturating_add(1);
            state.deadline = None;
            return;
        }

        let elapsed = now.sub(deadline).as_nanoseconds();
        let periods = elapsed / state.interval_ns;
        let expirations = periods.saturating_add(1);
        state.expirations = state.expirations.saturating_add(expirations);
        deadline = deadline.add_ns(expirations.saturating_mul(state.interval_ns));
        state.deadline = Some(deadline);
    }

    fn is_read_ready(&self) -> bool {
        let mut state = self.state.lock();
        Self::refresh(&mut state);
        state.expirations > 0
    }
}

impl Object for TimerFdObject {
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
    impl_cast_function_non_trait!("timerfd", TimerFdObject);
}

impl Pollable for TimerFdObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeRead) && self.is_read_ready()
    }
}

impl Readable for TimerFdObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        if buffer.len() < core::mem::size_of::<u64>() {
            return Err(ObjectError::InvalidArguments);
        }

        loop {
            let mut state = self.state.lock();
            Self::refresh(&mut state);

            if state.expirations > 0 {
                let expirations = state.expirations;
                state.expirations = 0;
                drop(state);

                buffer[..8].copy_from_slice(&expirations.to_ne_bytes());
                return Ok(8);
            }

            if self.flags.lock().contains(FileFlags::NONBLOCK) {
                return Err(ObjectError::TryAgain);
            }

            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: state.deadline,
            });
            drop(state);

            if self.is_read_ready() {
                cancel_block(&current);
                continue;
            }

            finish_block_current();
        }
    }
}

impl Statable for TimerFdObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
    }
}

pub fn wake_linux_io_waiters() {
    THREAD_MANAGER.get().unwrap().lock().wake_io();
}
