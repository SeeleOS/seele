use alloc::collections::vec_deque::VecDeque;
use spin::Mutex;

use crate::{keyboard::decoding_task::KEYBOARD_QUEUE, terminal::state::DEFAULT_TERMINAL};

lazy_static::lazy_static! {
    pub static ref LINE_BUFFER: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());
}
pub fn flush_line_buffer() {
    for ele in LINE_BUFFER.lock().drain(..) {
        KEYBOARD_QUEUE
            .get_or_init(|| Mutex::new(VecDeque::new()))
            .lock()
            .push_back(ele);
    }
}

pub fn clear() {
    DEFAULT_TERMINAL.get().unwrap().lock().inner.lock().clear();
}
