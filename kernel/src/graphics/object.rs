use core::{fmt::Write, str::from_utf8};

use crate::{
    graphics::{framebuffer::FRAME_BUFFER, terminal::TERMINAL},
    object::{Object, misc::ObjectResult, traits::Writable},
};

#[derive(Debug)]
pub struct TtyObject;

impl Object for TtyObject {
    fn as_writable(self: alloc::sync::Arc<Self>) -> Option<alloc::sync::Arc<dyn Writable>> {
        Some(self)
    }

    fn as_configuratable(
        self: alloc::sync::Arc<Self>,
    ) -> Option<alloc::sync::Arc<dyn crate::object::traits::Configuratable>> {
        Some(self)
    }
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
