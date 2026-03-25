use core::char;

use alloc::collections::vec_deque::VecDeque;
use spin::mutex::Mutex;

use crate::{
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::tty_device::wake_tty_poller_readable,
    print,
    terminal::{
        misc::{LINE_BUFFER, flush_line_buffer},
        state::DEFAULT_TERMINAL,
    },
    thread::THREAD_MANAGER,
};

pub fn process_char(char: char) {
    let info = *DEFAULT_TERMINAL.get().unwrap().lock().info.lock();

    if !info.canonical {
        // In noncanonical mode, userspace handles line editing and submission.
        if info.echo {
            print!("{char}");
        }

        KEYBOARD_QUEUE
            .get_or_init(|| Mutex::new(VecDeque::new()))
            .lock()
            .push_back(char as u8);
        THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
        wake_tty_poller_readable();

        return;
    }

    match char {
        '\n' => {
            if info.echo_newline {
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
                if info.echo_delete {
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
