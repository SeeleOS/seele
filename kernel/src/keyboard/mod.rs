use crate::keyboard::scancode_stream::{SCANCODE_QUEUE, WAKER};

pub mod char_processing;
pub mod decoding_task;
pub mod key_to_escape_sequence;
pub mod object;
pub mod ps2;
pub mod raw_key_processing;
pub mod scancode_stream;

pub fn init() {
    ps2::init();
}

pub(crate) fn push_scancode(scancode: u8) {
    let queue = SCANCODE_QUEUE.get().unwrap();
    queue.push(scancode).unwrap();

    // wake up the registered waker
    WAKER.wake();
    // Wakeup all the blocked process (it should be threads now lol)
}
