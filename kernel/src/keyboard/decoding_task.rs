use core::task::Poll;

use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use futures_util::{Stream, StreamExt, task::AtomicWaker};
use pc_keyboard::DecodedKey;
use spin::Mutex;

use crate::{
    keyboard::{ps2::_PS2_KEYBOARD, scancode_stream::ScancodeStream},
    multitasking::MANAGER,
    print, println,
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
            KEYBOARD_QUEUE
                .get_or_init(|| Mutex::new(VecDeque::new()))
                .lock()
                .push_back(character as u8);
        }
    }
}
