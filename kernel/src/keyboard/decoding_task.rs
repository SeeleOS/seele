use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use futures_util::StreamExt;
use pc_keyboard::DecodedKey;
use spin::Mutex;

use crate::keyboard::{
    char_processing::process_char, ps2::_PS2_KEYBOARD, raw_key_processing::process_key,
    scancode_stream::ScancodeStream,
};

pub static KEYBOARD_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();

pub async fn process_keypresses() {
    let mut scancodes = ScancodeStream::default();

    // loop through scancodes infinitely
    while let Some(scancode) = scancodes.next().await {
        let decoded_key = {
            let mut keyboard = _PS2_KEYBOARD.lock();

            if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
                keyboard.process_keyevent(key_event)
            } else {
                None
            }
        };

        if let Some(key) = decoded_key {
            match key {
                DecodedKey::RawKey(key_code) => process_key(key_code),
                DecodedKey::Unicode(character) => process_char(character),
            }
        }
    }
}
