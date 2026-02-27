use crate::object::error::ObjectError;

pub mod error;

pub trait Object: Send + Sync {}

pub type ObjectResult<T> = Result<T, ObjectError>;

pub trait Writable: Object {
    /// Write the content of [`buffer`] to [`self`]
    fn write(&self, buffer: &[u8]) -> ObjectResult<u64>;
}

pub trait Readable: Object {
    /// Reads the content of [`self`] and write them to [`buffer`]
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize>;
}
