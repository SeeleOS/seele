use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use futures_util::StreamExt;
use pc_keyboard::DecodedKey;
use spin::Mutex;

use crate::keyboard::{
    char_processing::process_char, encode_linux_raw_byte, ps2::_PS2_KEYBOARD,
    raw_key_processing::process_key, scancode_stream::ScancodeStream,
};
use crate::{
    evdev::push_keyboard_event,
    object::tty_device::{get_default_tty, wake_tty_poller_readable},
    terminal::linux_kd::{KeyboardMode, linux_keycode_from_keycode},
    thread::THREAD_MANAGER,
};
use pc_keyboard::{KeyEvent, KeyState};

pub static KEYBOARD_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();
/// Raw keyboard modes consume untranslated scancodes from this queue.
pub static RAW_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();
/// Medium raw mode consumes Linux keycodes with press/release state.
pub static MEDIUM_RAW_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();

pub async fn process_keypresses() {
    let mut scancodes = ScancodeStream::default();

    // loop through scancodes infinitely
    while let Some(scancode) = scancodes.next().await {
        RAW_QUEUE
            .get_or_init(|| Mutex::new(VecDeque::new()))
            .lock()
            .push_back(encode_linux_raw_byte(scancode));
        THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
        wake_tty_poller_readable();

        let decoded_key = {
            let mut keyboard = _PS2_KEYBOARD.lock();

            if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
                push_keyboard_event(&key_event);
                match get_default_tty().keyboard_mode() {
                    KeyboardMode::Raw | KeyboardMode::Off => continue,
                    KeyboardMode::MediumRaw => {
                        push_medium_raw_event(&key_event);
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

fn push_medium_raw_event(event: &KeyEvent) {
    let Some(keycode) = linux_keycode_from_keycode(event.code) else {
        return;
    };

    let mut queue = MEDIUM_RAW_QUEUE
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock();

    let released = matches!(event.state, KeyState::Up);
    if keycode < 0x80 {
        queue.push_back(keycode | if released { 0x80 } else { 0 });
    } else {
        queue.push_back(if released { 0x80 } else { 0 });
        queue.push_back((keycode >> 7) & 0x7f);
        queue.push_back(keycode & 0x7f);
    }
    drop(queue);

    THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
    wake_tty_poller_readable();
}
