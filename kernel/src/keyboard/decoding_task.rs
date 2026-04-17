use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use futures_util::StreamExt;
use pc_keyboard::DecodedKey;
use spin::Mutex;

use crate::keyboard::{
    char_processing::process_char, ps2::_PS2_KEYBOARD, raw_key_processing::process_key,
    scancode_stream::ScancodeStream,
};
use crate::{object::tty_device::get_default_tty, terminal::linux_kd::KeyboardMode};

pub static KEYBOARD_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();
/// Raw keyboard modes consume untranslated scancodes from this queue.
pub static RAW_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();

pub async fn process_keypresses() {
    let mut scancodes = ScancodeStream::default();

    // loop through scancodes infinitely
    while let Some(scancode) = scancodes.next().await {
        let decoded_key = {
            let mut keyboard = _PS2_KEYBOARD.lock();

            if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
                if keyboard_mode_blocks_terminal_decode() {
                    continue;
                }

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

fn keyboard_mode_blocks_terminal_decode() -> bool {
    let mode = get_default_tty().keyboard_mode();
    matches!(mode, KeyboardMode::Raw | KeyboardMode::Off)
}
