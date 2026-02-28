use core::fmt::Debug;

use alloc::sync::Arc;

use crate::{
    graphics::object_config::{TerminalInfo, WindowSizeInfo},
    object::error::ObjectError,
};

pub mod error;

pub trait Object: Send + Sync + Debug {
    fn as_writable(self: Arc<Self>) -> Option<Arc<dyn Writable>> {
        None
    }

    fn as_readable(self: Arc<Self>) -> Option<Arc<dyn Readable>> {
        None
    }
}

pub type ObjectResult<T> = Result<T, ObjectError>;

pub trait Writable: Object {
    /// Write the content of [`buffer`] to [`self`]
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize>;
}

pub trait Readable: Object {
    /// Reads the content of [`self`] and write them to [`buffer`]
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize>;
}

pub enum ConfigurateRequest {
    GetWindowSize(*mut WindowSizeInfo),
    GetTerminalInfo(*mut TerminalInfo),
}

pub trait Configuratable: Object {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize>;
}
