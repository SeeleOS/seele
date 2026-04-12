use alloc::{collections::vec_deque::VecDeque, sync::Arc};
use seele_sys::abi::object::ObjectFlags;
use spin::Mutex;

use crate::{
    impl_cast_function,
    object::{
        Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        queue_helpers::{copy_from_queue, read_or_block},
        traits::{Configuratable, Readable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    signal::Signal,
    terminal::line_discipline::{process_input_byte, process_output_bytes},
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
        let (master, slave, wrote_input, wrote_echo, interrupt_group) = {
            let mut shared = self.shared.lock();
            let master = shared.get_master();
            let slave = shared.get_slave();
            let info = shared.info;
            let mut wrote_input = false;
            let mut wrote_echo = false;
            let mut interrupt_group = None;

            for byte in buffer.iter().copied() {
                let mut queued_input = VecDeque::new();
                let mut queued_echo = VecDeque::new();
                let mut wants_interrupt = false;
                process_input_byte(
                    &info,
                    &mut shared.line_buffer,
                    byte,
                    |byte| {
                        queued_input.push_back(byte);
                    },
                    |bytes| {
                        process_output_bytes(&info, bytes, |byte| {
                            queued_echo.push_back(byte);
                        });
                    },
                    || {
                        wants_interrupt = true;
                    },
                );

                if wants_interrupt {
                    shared.line_buffer.clear();
                    interrupt_group = shared.active_group;
                }

                if !queued_input.is_empty() {
                    shared.from_master.append(&mut queued_input);
                    wrote_input = true;
                }

                if !queued_echo.is_empty() {
                    shared.from_slave.append(&mut queued_echo);
                    wrote_echo = true;
                }
            }

            (master, slave, wrote_input, wrote_echo, interrupt_group)
        };

        if let Some(group_id) = interrupt_group {
            group_id
                .get_processes()
                .iter()
                .for_each(|process| process.lock().send_signal(Signal::Interrupt));
        }

        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        manager.wake_pty();
        if wrote_input {
            manager.wake_poller(slave, PollableEvent::CanBeRead);
        }
        if wrote_echo {
            manager.wake_poller(master, PollableEvent::CanBeRead);
        }
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
        let slave = {
            let shared = self.shared.lock();
            shared.get_slave()
        };

        slave
            .as_configuratable()
            .map_err(|_| ObjectError::InvalidRequest)?
            .configure(request)
    }
}
