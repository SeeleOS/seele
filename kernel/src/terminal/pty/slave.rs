use alloc::sync::Arc;
use seele_sys::abi::object::{ObjectFlags, TerminalInfo};
use spin::Mutex;

use crate::{
    impl_cast_function,
    object::{
        Object,
        config::ConfigurateRequest,
        misc::ObjectResult,
        queue_helpers::{copy_from_queue, push_to_queue, read_or_block},
        traits::{Configuratable, Readable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::group::ProcessGroupID,
    terminal::pty::shared::PtyShared,
    thread::{THREAD_MANAGER, yielding::WakeType},
};

impl Pollable for PtySlave {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => !self.shared.lock().from_master.is_empty(),
            PollableEvent::CanBeWritten => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct PtySlave {
    shared: Arc<Mutex<PtyShared>>,
    info: Mutex<TerminalInfo>,
    /// The foreground process group currently attached to this tty.
    /// Line-discipline generated signals such as Ctrl+C should be sent here.
    pub active_group: Mutex<Option<ProcessGroupID>>,
    pub flags: Mutex<ObjectFlags>,
}

impl PtySlave {
    pub fn new(shared: Arc<Mutex<PtyShared>>) -> Self {
        Self {
            shared,
            info: Mutex::new(TerminalInfo::default()),
            active_group: Mutex::new(None),
            flags: Mutex::new(ObjectFlags::default()),
        }
    }
}

impl Object for PtySlave {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
}

impl Writable for PtySlave {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let master = {
            let mut shared = self.shared.lock();
            push_to_queue(&mut shared.from_slave, buffer);
            shared.get_master()
        };

        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        manager.wake_pty();
        manager.wake_poller(master, PollableEvent::CanBeRead);
        Ok(buffer.len())
    }
}

impl Readable for PtySlave {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        read_or_block(buffer, &self.flags, WakeType::Pty, |buffer| {
            let mut shared = self.shared.lock();
            if shared.from_master.is_empty() {
                None
            } else {
                Some(copy_from_queue(&mut shared.from_master, buffer))
            }
        })
    }
}

impl Configuratable for PtySlave {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::TermGetActiveGroup => {
                Ok(self.active_group.lock().unwrap().0 as isize)
            }
            ConfigurateRequest::TermSetActiveGroup(group) => {
                *self.active_group.lock() = Some(ProcessGroupID(group));
                Ok(0)
            }
            ConfigurateRequest::GetTerminalInfo(term_info) => unsafe {
                *term_info = *self.info.lock();
                Ok(0)
            },
            ConfigurateRequest::SetTerminalInfo(term_info) => unsafe {
                *self.info.lock() = *term_info;
                Ok(0)
            },
            _ => Ok(0),
        }
    }
}
