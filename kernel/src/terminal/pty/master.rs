use alloc::sync::Arc;
use seele_sys::abi::object::ObjectFlags;
use spin::Mutex;

use crate::{
    impl_cast_function,
    object::{
        Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        queue_helpers::{copy_from_queue, push_to_queue, read_or_block},
        traits::{Configuratable, Readable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    terminal::pty::shared::PtyShared,
    thread::{THREAD_MANAGER, yielding::WakeType},
};

impl Pollable for PtyMaster {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => !self.shared.lock().from_slave.is_empty(),
            PollableEvent::CanBeWritten => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct PtyMaster {
    shared: Arc<Mutex<PtyShared>>,
    pub flags: Mutex<ObjectFlags>,
}

impl PtyMaster {
    pub fn new(shared: Arc<Mutex<PtyShared>>) -> Self {
        Self {
            shared,
            flags: Mutex::new(ObjectFlags::default()),
        }
    }
}

impl Object for PtyMaster {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
}

impl Writable for PtyMaster {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let slave = {
            let mut shared = self.shared.lock();
            push_to_queue(&mut shared.from_master, buffer);
            shared.get_slave()
        };

        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        manager.wake_pty();
        manager.wake_poller(slave, PollableEvent::CanBeRead);
        Ok(buffer.len())
    }
}

impl Readable for PtyMaster {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        read_or_block(buffer, &self.flags, WakeType::Pty, |buffer| {
            let mut shared = self.shared.lock();
            if shared.from_slave.is_empty() {
                None
            } else {
                Some(copy_from_queue(&mut shared.from_slave, buffer))
            }
        })
    }
}

impl Configuratable for PtyMaster {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        self.shared
            .lock()
            .get_slave()
            .as_configuratable()
            .map_err(|_| ObjectError::InvalidRequest)?
            .configure(request)
    }
}
