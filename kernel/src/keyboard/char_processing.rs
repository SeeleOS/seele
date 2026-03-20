use core::char;

use alloc::collections::vec_deque::VecDeque;
use spin::mutex::Mutex;

use crate::{
    keyboard::decoding_task::KEYBOARD_QUEUE,
    multitasking::thread::THREAD_MANAGER,
    object::tty_device::wake_tty_poller_readable,
    print,
    terminal::{
        misc::{LINE_BUFFER, flush_line_buffer},
        state::DEFAULT_TERMINAL,
    },
};

pub fn process_char(char: char) {
    let info = *DEFAULT_TERMINAL.get().unwrap().lock().info.lock();

    if info.raw {
        // Under raw mode, you should not check if its a newline character,
        // delete character, and other bs, just echo it anyways as long as echo is enabled.
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

            // info.raw means the userspace program takes care of everything.
            // !info.raw means that i have to take care of the stuff
            if !info.raw {
                LINE_BUFFER.lock().push_back(b'\n');
                flush_line_buffer();
                THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
                wake_tty_poller_readable();
            }
        }
        '\x08' | '\x7f' => {
            if !info.raw {
                let mut lb = LINE_BUFFER.lock();
                if lb.pop_back().is_some() {
                    if info.echo_delete {
                        print!("\x08 \x08");
                    }
                }
            }
        }
        _ => {
            if info.echo {
                print!("{char}");
            }

            if !info.raw {
                LINE_BUFFER.lock().push_back(char as u8);
            }
        }
    }
}
