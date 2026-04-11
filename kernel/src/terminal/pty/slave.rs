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
        for b in buffer {
            self.shared.lock().from_slave.push_back(*b);
        }
        THREAD_MANAGER.get().unwrap().lock().wake_pty();
        Ok(buffer.len())
    }
}

impl Readable for PtySlave {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        loop {
            {
                let queue = &mut self.shared.lock().from_master;

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
