use core::str::from_utf8;

use alloc::collections::vec_deque::VecDeque;
use pc_keyboard::KeyCode;
use spin::mutex::Mutex;

use crate::{
    keyboard::{decoding_task::KEYBOARD_QUEUE, key_to_escape_sequence::to_escape_sequence},
    object::tty_device::wake_tty_poller_readable,
    print,
    terminal::state::DEFAULT_TERMINAL,
    thread::THREAD_MANAGER,
};

pub fn process_key(key: KeyCode) {
    let info = *DEFAULT_TERMINAL.get().unwrap().lock().info.lock();
    let sequence = to_escape_sequence(key);

    if !info.canonical {
        if info.echo
            && let Ok(str) = from_utf8(sequence)
        {
            print!("{str}");
        }

        for byte in sequence {
            KEYBOARD_QUEUE
                .get_or_init(|| Mutex::new(VecDeque::new()))
                .lock()
                .push_back(*byte);
        }

        THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
        wake_tty_poller_readable();

        return;
    }

    if info.echo
        && let Ok(str) = from_utf8(sequence)
    {
        print!("{str}");
    }
}
