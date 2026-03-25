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
    let mut keyboard = _PS2_KEYBOARD.lock();

    // loop through scancodes infinitely
    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode)
            && let Some(key) = keyboard.process_keyevent(key_event)
        {
            match key {
                DecodedKey::RawKey(key_code) => process_key(key_code),
                DecodedKey::Unicode(character) => process_char(character),
            }
        }
    }
}
