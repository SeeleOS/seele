use crate::{
    graphics::{object::TerminalObject, terminal::state::DEFAULT_TERMINAL},
    impl_cast_function,
    keyboard::object::KeyboardObject,
    object::{
        Object,
        traits::{Configuratable, Readable, Writable},
    },
};

#[derive(Debug)]
pub struct TtyDevice {
    terminal: TerminalObject,
}

impl Object for TtyDevice {
    impl_cast_function!(writable, Writable);
    impl_cast_function!(readable, Readable);
    impl_cast_function!(configuratable, Configuratable);
}

impl Configuratable for TtyDevice {
    fn configure(&self, request: super::config::ConfigurateRequest) -> super::ObjectResult<isize> {
        log::trace!("tty: configure");
        self.terminal.configure(request)
    }
}

impl Writable for TtyDevice {
    fn write(&self, buffer: &[u8]) -> super::ObjectResult<usize> {
        self.terminal.write(buffer)
    }
}

impl Readable for TtyDevice {
    fn read(&self, buffer: &mut [u8]) -> super::ObjectResult<usize> {
        KeyboardObject.read(buffer)
    }
}
