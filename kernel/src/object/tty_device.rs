use crate::{
    graphics::object::TtyObject,
    impl_cast_function,
    keyboard::object::KeyboardObject,
    object::{
        Object,
        traits::{Configuratable, Readable, Writable},
    },
};

#[derive(Debug)]
pub struct TtyDevice;

impl Object for TtyDevice {
    impl_cast_function!(writable, Writable);
    impl_cast_function!(readable, Readable);
    impl_cast_function!(configuratable, Configuratable);
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
