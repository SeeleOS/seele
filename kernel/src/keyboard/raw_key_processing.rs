use core::str::from_utf8;

use pc_keyboard::KeyCode;

use crate::{
    keyboard::key_to_escape_sequence::to_escape_sequence,
    object::tty_device::{get_active_tty, wake_tty_poller_readable},
    print,
    terminal::state::DEFAULT_TERMINAL,
    thread::THREAD_MANAGER,
};

pub fn process_key(key: KeyCode) {
    let info = *DEFAULT_TERMINAL.get().unwrap().lock().info.lock();
    let active_tty = get_active_tty();
    let sequence = to_escape_sequence(key);

    if !info.canonical {
        if info.echo
            && let Ok(str) = from_utf8(sequence)
        {
            print!("{str}");
        }

        active_tty.push_keyboard_bytes(sequence);

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
