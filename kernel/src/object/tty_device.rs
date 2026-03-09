use crate::{
    graphics::object::TtyObject,
    keyboard::object::KeyboardObject,
    object::{
        Object,
        traits::{Configuratable, Readable, Writable},
    },
};

#[derive(Debug)]
pub struct TtyDevice;

impl Object for TtyDevice {
    fn as_configuratable(
        self: alloc::sync::Arc<Self>,
    ) -> Option<alloc::sync::Arc<dyn super::traits::Configuratable>> {
        Some(self)
    }

    fn as_writable(self: alloc::sync::Arc<Self>) -> Option<alloc::sync::Arc<dyn super::Writable>> {
        Some(self)
    }

    fn as_readable(self: alloc::sync::Arc<Self>) -> Option<alloc::sync::Arc<dyn super::Readable>> {
        Some(self)
    }
}

impl Configuratable for TtyDevice {
    fn configure(&self, request: super::config::ConfigurateRequest) -> super::ObjectResult<isize> {
        TtyObject.configure(request)
    }
}

impl Writable for TtyDevice {
    fn write(&self, buffer: &[u8]) -> super::ObjectResult<usize> {
        TtyObject.write(buffer)
    }
}

impl Readable for TtyDevice {
    fn read(&self, buffer: &mut [u8]) -> super::ObjectResult<usize> {
        KeyboardObject.read(buffer)
    }
}
