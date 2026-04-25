use crate::{
    object::config::LinuxTermios2,
    object::tty_device::{get_active_tty, wake_tty_poller_readable},
    print,
    signal::{Signal, send_signal_to_process},
    terminal::{
        line_discipline::{process_input_byte, process_output_bytes},
        state::DEFAULT_TERMINAL,
    },
    thread::THREAD_MANAGER,
};

fn handle_interrupt_char(
    active_tty: &crate::object::tty_device::TtyDevice,
    termios: &LinuxTermios2,
) {
    if !termios.should_signal_on_special_chars() {
        return;
    }

    let active_group = *active_tty.active_group.lock();
    active_tty.clear_line_buffer();

    if let Some(group_id) = active_group {
        for process in group_id.get_processes() {
            send_signal_to_process(&process, Signal::Interrupt);
        }
    }

    THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
    wake_tty_poller_readable();
}

pub fn process_char(char: char) {
    let active_tty = get_active_tty();
    if !active_tty.receives_hardware_keyboard_input() {
        return;
    }

    let termios = *DEFAULT_TERMINAL.get().unwrap().lock().termios.lock();
    let Ok(byte) = u8::try_from(char as u32) else {
        return;
    };
    let queue_tty = active_tty.clone();
    let mut line_buffer = active_tty.line_buffer().lock();
    let mut wants_interrupt = false;

    process_input_byte(
        &termios,
        &mut line_buffer,
        byte,
        |byte| queue_tty.push_keyboard_byte(byte),
        |bytes| {
            let mut echoed = alloc::collections::vec_deque::VecDeque::new();
            process_output_bytes(&termios, bytes, |byte| {
                echoed.push_back(byte);
            });
            if let Ok(string) = core::str::from_utf8(echoed.make_contiguous()) {
                print!("{string}");
            }
        },
        || {
            wants_interrupt = true;
        },
    );
    drop(line_buffer);

    if wants_interrupt {
        handle_interrupt_char(&active_tty, &termios);
        return;
    }

    if byte == b'\n' {
        active_tty.flush_line_buffer();
    }
    THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
    wake_tty_poller_readable();
}
