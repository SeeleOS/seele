use core::char;

use crate::{
    multitasking::thread::THREAD_MANAGER,
    object::tty_device::wake_tty_poller_readable,
    print,
    terminal::{misc::{LINE_BUFFER, flush_line_buffer}, state::DEFAULT_TERMINAL},
};

pub fn process_char_non_raw(char: char) {
    let info = *DEFAULT_TERMINAL.get().unwrap().lock().info.lock();

    match char {
        '\n' => {
            if info.echo {
                print!("{char}");
            }
            LINE_BUFFER.lock().push_back(b'\n');
            flush_line_buffer();
            THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
            wake_tty_poller_readable();
        }
        '\x08' | '\x7f' => {
            let mut lb = LINE_BUFFER.lock();
            if lb.pop_back().is_some() {
                if info.echo {
                    print!("\x08 \x08");
                }
            }
        }
        _ => {
            if info.echo {
                print!("{char}");
            }
            LINE_BUFFER.lock().push_back(char as u8);
        }
    }
}
