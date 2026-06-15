use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    memory::utils::Mut,
    misc::time::Time,
    object::{
        FileFlags, Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        traits::{Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        wake_pollers_for_object,
    },
};

#[derive(Default)]
struct TimerFdRegistry {
    armed: BTreeMap<(Time, usize), Weak<TimerFdObject>>,
}

lazy_static::lazy_static! {
    static ref TIMERFD_REGISTRY: Mut<TimerFdRegistry> = Mut::new(TimerFdRegistry::default());
}

#[derive(Debug, Clone, Copy, Default)]
struct TimerFdState {
    deadline: Option<Time>,
    interval_ns: u64,
    expirations: u64,
}

#[derive(Debug, Default)]
pub struct TimerFdObject {
    flags: Mut<FileFlags>,
    state: Mut<TimerFdState>,
    self_ref: Mut<Option<Weak<TimerFdObject>>>,
}

impl TimerFdObject {
    pub fn new(flags: FileFlags) -> Arc<Self> {
        let timerfd = Arc::new(Self {
            flags: Mut::new(flags),
            state: Mut::new(TimerFdState::default()),
            self_ref: Mut::new(None),
        });
        *timerfd.self_ref.lock() = Some(Arc::downgrade(&timerfd));
        timerfd
    }

    pub fn set_timer(&self, deadline: Option<Time>, interval_ns: u64) {
        let mut state = self.state.lock();
        let previous_deadline = state.deadline;
        state.deadline = deadline;
        state.interval_ns = interval_ns;
        state.expirations = 0;
        drop(state);
        self.update_registry(previous_deadline, deadline);
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

    fn refresh_state(&self) -> TimerFdState {
        let (previous_deadline, state) = {
            let mut state = self.state.lock();
            let previous_deadline = state.deadline;
            Self::refresh(&mut state);
            (previous_deadline, *state)
        };
        self.update_registry(previous_deadline, state.deadline);
        state
    }

    pub fn is_read_ready(&self) -> bool {
        self.refresh_state().expirations > 0
    }

    fn self_object(&self) -> Option<ObjectRef> {
        self.self_ref
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(|object| object as ObjectRef)
    }

    fn self_timerfd(&self) -> Option<Arc<Self>> {
        self.self_ref.lock().as_ref().and_then(Weak::upgrade)
    }

    fn update_registry(&self, previous_deadline: Option<Time>, new_deadline: Option<Time>) {
        if previous_deadline == new_deadline {
            return;
        }

        let Some(timerfd) = self.self_timerfd() else {
            return;
        };
        update_timerfd_deadline(&timerfd, previous_deadline, new_deadline);
    }

    pub fn wake_waiters(&self) {
        if let Some(object) = self.self_object() {
            wake_pollers_for_object(object, PollableEvent::CanBeRead);
        }
    }
}

fn timerfd_key(timerfd: &Arc<TimerFdObject>) -> usize {
    Arc::as_ptr(timerfd) as usize
}

fn update_timerfd_deadline(
    timerfd: &Arc<TimerFdObject>,
    previous_deadline: Option<Time>,
    new_deadline: Option<Time>,
) {
    let mut registry = TIMERFD_REGISTRY.lock();
    let key = timerfd_key(timerfd);

    if let Some(previous_deadline) = previous_deadline {
        registry.armed.remove(&(previous_deadline, key));
    }

    if let Some(new_deadline) = new_deadline {
        registry
            .armed
            .insert((new_deadline, key), Arc::downgrade(timerfd));
    }
}

fn expired_timerfds(now: Time) -> Vec<Arc<TimerFdObject>> {
    let mut registry = TIMERFD_REGISTRY.lock();
    let mut strong = Vec::new();
    while let Some((&(deadline, _), _)) = registry.armed.first_key_value() {
        if deadline > now {
            break;
        }

        let Some((_, watcher)) = registry.armed.pop_first() else {
            break;
        };

        if let Some(timerfd) = watcher.upgrade() {
            strong.push(timerfd);
        }
    }
    strong
}

pub fn expired_timerfd_poll_objects() -> Vec<ObjectRef> {
    expired_timerfds(Time::since_boot())
        .into_iter()
        .filter(|timerfd| timerfd.is_read_ready())
        .filter_map(|timerfd| timerfd.self_object())
        .collect()
}

pub fn next_timerfd_poll_deadline() -> Option<Time> {
    TIMERFD_REGISTRY
        .lock()
        .armed
        .first_key_value()
        .map(|((deadline, _), _)| *deadline)
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
            let state = self.refresh_state();
            if state.expirations > 0 {
                let expirations = {
                    let mut state = self.state.lock();
                    let expirations = state.expirations;
                    state.expirations = 0;
                    expirations
                };

                buffer[..8].copy_from_slice(&expirations.to_ne_bytes());
                return Ok(8);
            }

            let deadline = state.deadline;

            if self.flags.lock().contains(FileFlags::NONBLOCK) {
                return Err(ObjectError::TryAgain);
            }

            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline,
            });

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
    crate::thread::with_thread_manager(|manager| manager.wake_io());
}
