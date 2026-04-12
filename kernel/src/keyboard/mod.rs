use crate::{
    keyboard::{
        decoding_task::RAW_QUEUE,
        scancode_stream::{SCANCODE_QUEUE, WAKER},
    },
    object::tty_device::wake_tty_poller_readable,
    thread::THREAD_MANAGER,
};
use alloc::collections::vec_deque::VecDeque;
use spin::Mutex;

pub mod char_processing;
pub mod decoding_task;
pub mod key_to_escape_sequence;
pub mod ps2;
pub mod raw_key_processing;
pub mod scancode_stream;

fn encode_linux_raw_byte(scancode: u8) -> u8 {
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
    RAW_QUEUE
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .push_back(encode_linux_raw_byte(scancode));

    // wake up the registered waker
    WAKER.wake();
    THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
    wake_tty_poller_readable();
    // Wakeup all the blocked process (it should be threads now lol)
}
