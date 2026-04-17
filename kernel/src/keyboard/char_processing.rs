use core::char;

use alloc::collections::vec_deque::VecDeque;
use spin::mutex::Mutex;

use crate::{
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::tty_device::{get_default_tty, wake_tty_poller_readable},
    print,
    signal::Signal,
    terminal::{
        line_discipline::{process_input_byte, process_output_bytes},
        misc::{LINE_BUFFER, flush_line_buffer},
        object::TerminalSettings,
        state::DEFAULT_TERMINAL,
    },
    thread::THREAD_MANAGER,
};

fn handle_interrupt_char(info: &TerminalSettings) {
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
    let Ok(byte) = u8::try_from(char as u32) else {
        return;
    };

    process_input_byte(
        &info,
        &mut LINE_BUFFER.lock(),
        byte,
        |byte| {
            KEYBOARD_QUEUE
                .get_or_init(|| Mutex::new(VecDeque::new()))
                .lock()
                .push_back(byte);
        },
        |bytes| {
            let mut echoed = VecDeque::new();
            process_output_bytes(&info, bytes, |byte| {
                echoed.push_back(byte);
            });
            if let Ok(string) = core::str::from_utf8(echoed.make_contiguous()) {
                print!("{string}");
            }
        },
        || handle_interrupt_char(&info),
    );

    if byte == b'\n' {
        flush_line_buffer();
    }
    THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
    wake_tty_poller_readable();
}
