use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::{
    impl_cast_function,
    keyboard::decoding_task::KEYBOARD_QUEUE,
    keyboard::object::KeyboardObject,
    multitasking::thread::THREAD_MANAGER,
    object::{
        Object,
        misc::ObjectRef,
        traits::{Configuratable, Controllable, Readable, Writable},
    },
    polling::event::PollableEvent,
    terminal::object::TerminalObject,
};

pub static DEFAULT_TTY: OnceCell<Arc<TtyDevice>> = OnceCell::uninit();

pub fn get_default_tty() -> Arc<TtyDevice> {
    DEFAULT_TTY.get().unwrap().clone()
}

// Is the tty already readable? (for polling)
pub fn is_tty_readable(object: &ObjectRef) -> bool {
    let default_tty: ObjectRef = get_default_tty();
    Arc::ptr_eq(object, &default_tty)
        && !KEYBOARD_QUEUE
            .get_or_init(|| Mutex::new(Default::default()))
            .lock()
            .is_empty()
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
}

impl TtyDevice {
    pub fn new(terminal: Arc<Mutex<TerminalObject>>) -> Self {
        Self { terminal }
    }
}

impl Object for TtyDevice {
    impl_cast_function!(writable, Writable);
    impl_cast_function!(readable, Readable);
    impl_cast_function!(configuratable, Configuratable);
    impl_cast_function!(controllable, Controllable);
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
        KeyboardObject.read(buffer)
    }
}

impl Controllable for TtyDevice {
    fn control(
        &self,
        _command: super::control::Command,
        _arg: u64,
    ) -> super::misc::ObjectResult<isize> {
        // Stub
        Ok(0)
    }
}
