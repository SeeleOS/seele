use core::char;

use crate::{
    multitasking::thread::THREAD_MANAGER,
    print,
    terminal::misc::{LINE_BUFFER, flush_line_buffer},
};

pub fn process_char_non_raw(char: char) {
    match char {
        '\n' => {
            print!("{char}");
            LINE_BUFFER.lock().push_back(b'\n');
            flush_line_buffer();
            THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
        }
        '\x08' | '\x7f' => {
            let mut lb = LINE_BUFFER.lock();
            if lb.pop_back().is_some() {
                print!("\x08 \x08");
            }
        }
        _ => {
            print!("{char}");
            LINE_BUFFER.lock().push_back(char as u8);
        }
    }
}
