use alloc::{collections::vec_deque::VecDeque, sync::Arc};
use conquer_once::spin::OnceCell;
use seele_sys::abi::object::{ObjectFlags, TerminalInfo};
use spin::Mutex;

use crate::{
    impl_cast_function,
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::{
        Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        traits::{Configuratable, Readable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::{group::ProcessGroupID, manager::get_current_process},
    s_println,
    terminal::{object::TerminalObject, pty::shared::PtyShared},
    thread::{
        THREAD_MANAGER,
        yielding::{BlockType, WakeType, block_current, block_current_with_sig_check},
    },
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
        for b in buffer {
            self.shared.lock().from_master.push_back(*b);
        }
        THREAD_MANAGER.get().unwrap().lock().wake_pty();
        Ok(buffer.len())
    }
}

impl Readable for PtyMaster {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        loop {
            {
                let queue = &mut self.shared.lock().from_slave;

                if queue.is_empty() {
                    if self.flags.lock().contains(ObjectFlags::NONBLOCK) {
                        return Err(ObjectError::TryAgain);
                    }
                } else {
                    let mut read_chars = 0;
                    while read_chars < buffer.len() {
                        match queue.pop_front() {
                            Some(val) => {
                                buffer[read_chars] = val;
                                read_chars += 1;
                            }
                            None => break,
                        }
                    }

                    return Ok(read_chars);
                }
            }

            block_current_with_sig_check(BlockType::WakeRequired {
                wake_type: WakeType::Pty,
                deadline: None,
            })?;
        }
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
