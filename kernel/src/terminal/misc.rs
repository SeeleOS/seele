use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::keyboard::decoding_task::KEYBOARD_QUEUE;

lazy_static::lazy_static! {
    pub static ref LINE_BUFFER: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());
}
pub fn flush_line_buffer() {
    for ele in LINE_BUFFER.lock().drain(..) {
        KEYBOARD_QUEUE.get().unwrap().lock().push_back(ele);
    }
}
