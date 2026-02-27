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
    println,
};

pub static KEYBOARD_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();

pub async fn process_keypresses() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = _PS2_KEYBOARD.lock();

    // loop through scancodes infinitely
    while let Some(scancode) = scancodes.next().await {
        let test_key_event = keyboard.add_byte(scancode);

        if let Ok(Some(key_event)) = test_key_event {
            let thing = keyboard.process_keyevent(key_event);
            if let Some(key) = thing {
                match key {
                    pc_keyboard::DecodedKey::Unicode(character) => {
                        KEYBOARD_QUEUE
                            .get_or_init(|| Mutex::new(VecDeque::new()))
                            .lock()
                            .push_back(character as u8);
                        println!("{character}");
                    }
                    DecodedKey::RawKey(key) => {}
                }
            }
        }
    }
}
