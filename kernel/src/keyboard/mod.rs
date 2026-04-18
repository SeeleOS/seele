use crate::keyboard::scancode_stream::{SCANCODE_QUEUE, WAKER};

pub mod char_processing;
pub mod decoding_task;
pub mod key_to_escape_sequence;
pub mod ps2;
pub mod raw_key_processing;
pub mod scancode_stream;

pub(super) fn encode_linux_raw_byte(scancode: u8) -> u8 {
    match scancode {
        0xE0 => 0x60,
        0xE1 => 0x61,
        other => other,
    }
}

pub fn init() {
    SCANCODE_QUEUE
        .try_init_once(|| crossbeam_queue::ArrayQueue::new(512))
        .expect("keyboard scancode queue initialized twice");
    ps2::init();
}

pub(crate) fn push_scancode(scancode: u8) {
    let queue = SCANCODE_QUEUE.get().unwrap();
    let _ = queue.push(scancode);

    // Defer queue consumers to task context instead of doing thread wakeups on the IRQ stack.
    WAKER.wake();
}
