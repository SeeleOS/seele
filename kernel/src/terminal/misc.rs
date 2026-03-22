use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::{
    keyboard::decoding_task::KEYBOARD_QUEUE, println, s_println, terminal::state::DEFAULT_TERMINAL,
};

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
    let rows = DEFAULT_TERMINAL.get().unwrap().lock().info.lock().rows;
    for _ in 0..rows {
        println!();
    }
}
