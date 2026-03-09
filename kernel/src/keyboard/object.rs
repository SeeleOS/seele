use alloc::collections::vec_deque::VecDeque;
use spin::mutex::Mutex;

use crate::{
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::{Object, misc::ObjectResult, traits::Readable},
};

#[derive(Debug)]
pub struct KeyboardObject;

impl Object for KeyboardObject {
    fn as_readable(self: alloc::sync::Arc<Self>) -> Option<alloc::sync::Arc<dyn Readable>> {
        Some(self)
    }
}

impl Readable for KeyboardObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        let mut queue = KEYBOARD_QUEUE
            .get_or_init(|| Mutex::new(VecDeque::new()))
            .lock();

        if queue.is_empty() {
            return Ok(0);
        }

        let mut read_chars = 0;
        while read_chars < buffer.len() {
            if let Some(result) = queue.pop_front() {
                buffer[read_chars] = result;
                read_chars += 1;
            }
        }

        Ok(read_chars)
    }
}
