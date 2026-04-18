use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    misc::time::Time,
    object::{
        FileFlags, Object,
        error::ObjectError,
        misc::ObjectResult,
        traits::{Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    thread::{
        THREAD_MANAGER,
        yielding::{
            BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        },
    },
};

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
