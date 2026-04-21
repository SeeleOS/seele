use futures_util::StreamExt;
use pc_keyboard::DecodedKey;

use crate::keyboard::{
    char_processing::process_char, encode_linux_raw_byte, ps2::_PS2_KEYBOARD,
    raw_key_processing::process_key, scancode_stream::ScancodeStream,
};
use crate::{
    evdev::push_keyboard_event,
    object::tty_device::{get_active_tty, wake_tty_poller_readable},
    terminal::linux_kd::{KeyboardMode, linux_keycode_from_keycode},
    thread::THREAD_MANAGER,
};
use pc_keyboard::{KeyEvent, KeyState};

pub async fn process_keypresses() {
    let mut scancodes = ScancodeStream;

    // loop through scancodes infinitely
    while let Some(scancode) = scancodes.next().await {
        let active_tty = get_active_tty();
        active_tty.push_raw_byte(encode_linux_raw_byte(scancode));
        THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
        wake_tty_poller_readable();

        let decoded_key = {
            let mut keyboard = _PS2_KEYBOARD.lock();

            if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
                push_keyboard_event(&key_event);
                match active_tty.keyboard_mode() {
                    KeyboardMode::Raw | KeyboardMode::Off => continue,
                    KeyboardMode::MediumRaw => {
                        push_medium_raw_event(&active_tty, &key_event);
                        continue;
                    }
                    KeyboardMode::Xlate | KeyboardMode::Unicode => {}
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

fn push_medium_raw_event(active_tty: &crate::object::tty_device::TtyDevice, event: &KeyEvent) {
    let Some(keycode) = linux_keycode_from_keycode(event.code) else {
        return;
    };

    let released = matches!(event.state, KeyState::Up);
    let mut encoded = [0u8; 3];
    let len = if keycode < 0x80 {
        encoded[0] = keycode | if released { 0x80 } else { 0 };
        1
    } else {
        encoded[0] = if released { 0x80 } else { 0 };
        encoded[1] = (keycode >> 7) & 0x7f;
        encoded[2] = keycode & 0x7f;
        3
    };
    active_tty.push_medium_raw_bytes(&encoded[..len]);

    THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
    wake_tty_poller_readable();
}
