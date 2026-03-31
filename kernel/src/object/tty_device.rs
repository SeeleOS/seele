use alloc::{collections::vec_deque::VecDeque, sync::Arc};
use conquer_once::spin::OnceCell;
use seele_sys::abi::object::ObjectFlags;
use spin::Mutex;

use crate::{
    impl_cast_function,
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::{
        Object,
        control::ControlRequest,
        error::ObjectError,
        misc::ObjectRef,
        traits::{Configuratable, Controllable, Readable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::group::ProcessGroupID,
    terminal::object::TerminalObject,
    thread::{
        THREAD_MANAGER,
        yielding::{BlockType, WakeType, block_current},
    },
};

pub static DEFAULT_TTY: OnceCell<Arc<TtyDevice>> = OnceCell::uninit();

pub fn get_default_tty() -> Arc<TtyDevice> {
    DEFAULT_TTY.get().unwrap().clone()
}

impl Pollable for TtyDevice {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => !KEYBOARD_QUEUE
                .get_or_init(|| Mutex::new(Default::default()))
                .lock()
                .is_empty(),
            PollableEvent::CanBeWritten => true,
            _ => false,
        }
    }
}

pub fn wake_tty_poller_readable() {
    let tty: ObjectRef = get_default_tty();
    THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .wake_poller(tty, PollableEvent::CanBeRead);
}

#[derive(Debug)]
pub struct TtyDevice {
    terminal: Arc<Mutex<TerminalObject>>,
    /// The foreground process group currently attached to this tty.
    /// Line-discipline generated signals such as Ctrl+C should be sent here.
    pub active_group: Mutex<Option<ProcessGroupID>>,
    pub flags: Mutex<ObjectFlags>,
}

impl TtyDevice {
    pub fn new(terminal: Arc<Mutex<TerminalObject>>) -> Self {
        Self {
            terminal,
            active_group: Mutex::new(None),
            flags: Mutex::new(ObjectFlags::empty()),
        }
    }
}

impl Object for TtyDevice {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("controllable", Controllable);
    impl_cast_function!("pollable", Pollable);
}

impl Configuratable for TtyDevice {
    fn configure(&self, request: super::config::ConfigurateRequest) -> super::ObjectResult<isize> {
        log::trace!("tty: configure");
        self.terminal.lock().configure(request)
    }
}

impl Writable for TtyDevice {
    fn write(&self, buffer: &[u8]) -> super::ObjectResult<usize> {
        self.terminal.lock().write(buffer)
    }
}

impl Readable for TtyDevice {
    fn read(&self, buffer: &mut [u8]) -> super::ObjectResult<usize> {
        loop {
            let mut queue = KEYBOARD_QUEUE
                .get_or_init(|| Mutex::new(VecDeque::new()))
                .lock();

            if queue.is_empty() {
                if self.flags.lock().contains(ObjectFlags::NONBLOCK) {
                    return Err(ObjectError::TryAgain);
                }

                drop(queue);
                block_current(BlockType::WakeRequired {
                    wake_type: WakeType::Keyboard,
                    deadline: None,
                });
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
    }
}

impl Controllable for TtyDevice {
    fn control(&self, request: ControlRequest) -> super::misc::ObjectResult<isize> {
        match request {
            ControlRequest::GetFlags => Ok(self.flags.lock().bits() as isize),
            ControlRequest::SetFlags(flags) => {
                *self.flags.lock() = flags;

                Ok(0)
            }
        }
    }
}
