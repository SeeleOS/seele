use core::str::from_utf8;

use crate::{
    graphics::tty::Tty,
    object::{Object, Writable},
};

impl Writable for Tty {
    fn write(&self, buffer: &[u8]) -> crate::object::ObjectResult<usize> {
        self.print_string(from_utf8(buffer).unwrap());

        Ok(buffer.len())
    }
}
