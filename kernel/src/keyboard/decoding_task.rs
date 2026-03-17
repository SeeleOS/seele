use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::{Once, OnceCell};
use futures_util::StreamExt;
use pc_keyboard::DecodedKey;
use spin::Mutex;

use crate::{
    keyboard::{ps2::_PS2_KEYBOARD, scancode_stream::ScancodeStream},
    print,
    terminal::{
        misc::{LINE_BUFFER, flush_line_buffer},
        state::DEFAULT_TERMINAL,
    },
};

pub static KEYBOARD_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();

pub async fn process_keypresses() {
    let mut scancodes = ScancodeStream::default();
    let mut keyboard = _PS2_KEYBOARD.lock();

    // loop through scancodes infinitely
    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode)
            && let Some(key) = keyboard.process_keyevent(key_event)
            && let DecodedKey::Unicode(character) = key
        {
            if DEFAULT_TERMINAL
                .get()
                .unwrap()
                .lock()
                .terminal_info
                .lock()
                .is_raw_mode()
            {
                KEYBOARD_QUEUE
                    .get_or_init(|| Mutex::new(VecDeque::new()))
                    .lock()
                    .push_back(character as u8);
            } else {
                print!("{character}");

                LINE_BUFFER.get().unwrap().lock().push_back(character as u8);

                if character == '\n' {
                    flush_line_buffer();
                }
            }
        }
    }
}
