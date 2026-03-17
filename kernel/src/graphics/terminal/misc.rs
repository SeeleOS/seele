use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::keyboard::decoding_task::KEYBOARD_QUEUE;

pub static LINE_BUFFER: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();

pub fn flush_line_buffer() {
    for ele in LINE_BUFFER.get().unwrap().lock().drain(..) {
        KEYBOARD_QUEUE.get().unwrap().lock().push_back(ele);
    }
}
