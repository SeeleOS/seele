use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use futures_util::StreamExt;
use pc_keyboard::DecodedKey;
use spin::Mutex;

use crate::{
    keyboard::{
        char_processing::process_char, ps2::_PS2_KEYBOARD,
        raw_key_processing::raw_key_to_escape_sequence, scancode_stream::ScancodeStream,
    },
    multitasking::thread::THREAD_MANAGER,
    object::tty_device::wake_tty_poller_readable,
};

pub static KEYBOARD_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();

pub async fn process_keypresses() {
    let mut scancodes = ScancodeStream::default();
    let mut keyboard = _PS2_KEYBOARD.lock();

    // loop through scancodes infinitely
    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode)
            && let Some(key) = keyboard.process_keyevent(key_event)
        {
            match key {
                DecodedKey::RawKey(key_code) => {
                    let sequence = raw_key_to_escape_sequence(key_code);
                    for b in sequence {
                        KEYBOARD_QUEUE
                            .get_or_init(|| Mutex::new(VecDeque::new()))
                            .lock()
                            .push_back(*b);
                    }

                    if !sequence.is_empty() {
                        THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
                        wake_tty_poller_readable();
                    }
                }
                DecodedKey::Unicode(character) => process_char(character),
            }
        }
    }
}
