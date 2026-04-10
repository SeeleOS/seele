use core::char;

use alloc::collections::vec_deque::VecDeque;
use bootloader_api::info;
use seele_sys::{abi::object::TerminalInfo, signal::Signal};
use spin::mutex::Mutex;

use crate::{
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::tty_device::{get_default_tty, wake_tty_poller_readable},
    print,
    terminal::{
        misc::{LINE_BUFFER, flush_line_buffer},
        state::DEFAULT_TERMINAL,
    },
    thread::THREAD_MANAGER,
};

fn handle_interrupt_char(info: &TerminalInfo) {
    if let Some(group_id) = *get_default_tty().active_group.lock()
        && info.send_sig_on_special_chars
    {
        LINE_BUFFER.lock().clear();
        group_id
            .get_processes()
            .iter()
            .for_each(|process| process.lock().send_signal(Signal::Interrupt));
        THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
        wake_tty_poller_readable();
    }
}

pub fn process_char(char: char) {
    let info = *DEFAULT_TERMINAL.get().unwrap().lock().info.lock();

    if char == '\x03' {
        handle_interrupt_char(&info);
        return;
    }

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
        '\x03' => handle_interrupt_char(&info),
        _ => {
            if info.echo {
                print!("{char}");
            }

            LINE_BUFFER.lock().push_back(char as u8);
        }
    }
}
