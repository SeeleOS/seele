use alloc::collections::vec_deque::VecDeque;
use spin::mutex::Mutex;

use crate::{
    impl_cast_function,
    keyboard::decoding_task::KEYBOARD_QUEUE,
    multitasking::thread::{
        THREAD_MANAGER,
        yielding::{BlockType, WakeType, block_current},
    },
    object::{Object, misc::ObjectResult, traits::Readable},
};

#[derive(Debug)]
pub struct KeyboardObject;

impl Object for KeyboardObject {
    impl_cast_function!(readable, Readable);
}

impl Readable for KeyboardObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        loop {
            let mut queue = KEYBOARD_QUEUE
                .get_or_init(|| Mutex::new(VecDeque::new()))
                .lock();

            if queue.is_empty() {
                drop(queue);
                block_current(BlockType::WakeRequired(WakeType::Keyboard));
            } else {
                let mut read_chars = 0;
                while read_chars < buffer.len() {
                    if let Some(result) = queue.pop_front() {
                        buffer[read_chars] = result;
                        read_chars += 1;
                    }
                }

                return Ok(read_chars);
            }
        }
    }
}
