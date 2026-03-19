use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::{
    impl_cast_function,
    keyboard::object::KeyboardObject,
    object::{
        Object,
        traits::{Configuratable, Controllable, Readable, Writable},
    },
    terminal::{
        object::TerminalObject,
        state::{self, DEFAULT_TERMINAL},
    },
};

pub static DEFAULT_TTY: OnceCell<Arc<TtyDevice>> = OnceCell::uninit();

pub fn get_default_tty() -> Arc<TtyDevice> {
    DEFAULT_TTY.get().unwrap().clone()
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
    fn control(&self, command: super::control::Command) -> super::misc::ObjectResult<isize> {
        // Stub
        Ok(0)
    }
}
