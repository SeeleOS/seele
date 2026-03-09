use core::{fmt::Write, str::from_utf8};

use crate::{
    graphics::{framebuffer::FRAME_BUFFER, terminal::TERMINAL},
    impl_cast_function,
    object::{
        Object,
        misc::ObjectResult,
        traits::{Configuratable, Writable},
    },
};

#[derive(Debug)]
pub struct TtyObject;

impl Object for TtyObject {
    impl_cast_function!(configuratable, Configuratable);
    impl_cast_function!(writable, Writable);
}

impl Writable for TtyObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let mut terminal = TERMINAL.get().unwrap().lock();

        terminal
            .write_str(from_utf8(buffer).unwrap_or("Unsupported character"))
            .unwrap();

        FRAME_BUFFER.get().unwrap().lock().flush();

        Ok(buffer.len())
    }
}
