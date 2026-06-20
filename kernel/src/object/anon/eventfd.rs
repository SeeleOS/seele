use alloc::sync::{Arc, Weak};

use bitflags::bitflags;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    memory::utils::Mut,
    object::{
        FileFlags, Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        traits::{Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        wake_pollers_for_object,
    },
};

const EVENTFD_COUNTER_MAX: u64 = u64::MAX - 1;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct EventFdFlags: i32 {
        const EFD_SEMAPHORE = 0x1;
        const EFD_NONBLOCK = 0o4_000;
        const EFD_CLOEXEC = 0o2_000_000;
    }
}

impl EventFdFlags {
    pub fn object_flags(self) -> FileFlags {
        if self.contains(Self::EFD_NONBLOCK) {
            FileFlags::NONBLOCK
        } else {
            FileFlags::empty()
        }
    }
}

#[derive(Debug)]
struct EventFdState {
    counter: u64,
}

#[derive(Debug)]
pub struct EventFdObject {
    flags: Mut<FileFlags>,
    state: Mut<EventFdState>,
    semaphore: bool,
    self_ref: Mut<Option<Weak<EventFdObject>>>,
}

impl EventFdObject {
    pub fn new(initial: u64, flags: EventFdFlags) -> Arc<Self> {
        let eventfd = Arc::new(Self {
            flags: Mut::new(flags.object_flags()),
            state: Mut::new(EventFdState { counter: initial }),
            semaphore: flags.contains(EventFdFlags::EFD_SEMAPHORE),
            self_ref: Mut::new(None),
        });
        {
            let mut self_ref = eventfd.self_ref.lock();
            *self_ref = Some(Arc::downgrade(&eventfd));
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
        crate::thread::with_thread_manager(|manager| {
            manager.wake_io();
        });
        if let Some(object) = self.self_object() {
            wake_pollers_for_object(object, event);
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
