use alloc::collections::vec_deque::VecDeque;
use spin::mutex::Mutex;

use crate::{
    impl_cast_function,
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::{Object, misc::ObjectResult, traits::Readable},
    thread::yielding::{BlockType, WakeType, block_current},
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
                block_current(BlockType::WakeRequired {
                    wake_type: WakeType::Keyboard,
                });
            } else {
                let mut read_chars = 0;
                while read_chars < buffer.len() {
                    match queue.pop_front() {
                        Some(val) => {
                            buffer[read_chars] = val;
                            read_chars += 1;
                        }
                        None => break,
                    }
                }

                return Ok(read_chars);
            }
        }
    }
}
