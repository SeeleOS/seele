use x86_64::registers::mxcsr::read;

use crate::{
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::{Object, Readable, error::ObjectError},
};

#[derive(Debug)]
pub struct KeyboardObject;

impl Object for KeyboardObject {
    fn as_readable(
        self: alloc::sync::Arc<Self>,
    ) -> Option<alloc::sync::Arc<dyn crate::object::Readable>> {
        Some(self)
    }
}

impl Readable for KeyboardObject {
    fn read(&self, buffer: &mut [u8]) -> crate::object::ObjectResult<usize> {
        let queue = KEYBOARD_QUEUE.get().unwrap().lock();

        if queue.is_empty() {
            return Err(ObjectError::Other);
        }

        let mut read_chars = 0;
        for char in queue.iter() {
            buffer[read_chars] = char.clone();
            read_chars += 1;
        }

        Ok(read_chars)
    }
}
