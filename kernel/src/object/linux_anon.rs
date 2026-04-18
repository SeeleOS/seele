use alloc::sync::{Arc, Weak};
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
    thread::{
        THREAD_MANAGER,
        yielding::{
            BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        },
    },
};

const EVENTFD_SEMAPHORE: i32 = 0x1;
const EVENTFD_COUNTER_MAX: u64 = u64::MAX - 1;

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
