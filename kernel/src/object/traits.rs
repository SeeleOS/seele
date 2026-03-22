use crate::object::{Object, config::ConfigurateRequest, control, misc::ObjectResult};

pub trait Writable: Object {
    /// Write the content of [`buffer`] to [`self`]
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize>;
}

pub trait Readable: Object {
    /// Reads the content of [`self`] and write them to [`buffer`]
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize>;
}

pub trait Configuratable: Object {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize>;
}

pub trait Controllable: Object {
    fn control(&self, command: control::Command, arg: u64) -> ObjectResult<isize>;
}
