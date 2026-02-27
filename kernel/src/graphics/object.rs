use core::str::from_utf8;

use crate::{
    graphics::tty::{TTY, Tty},
    object::{Object, Writable},
};

#[derive(Debug)]
pub struct TtyObject;

impl Object for TtyObject {}
impl Writable for TtyObject {
    fn write(&self, buffer: &[u8]) -> crate::object::ObjectResult<usize> {
        let mut tty = TTY.get().unwrap().lock();

        tty.print_string(from_utf8(buffer).unwrap_or("Unsupported character"));

        Ok(buffer.len())
    }
}
