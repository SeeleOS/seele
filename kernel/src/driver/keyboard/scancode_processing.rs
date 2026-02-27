use core::task::Poll;

use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use futures_util::{Stream, StreamExt, task::AtomicWaker};
use spin::Mutex;

use crate::{
    driver::keyboard::ps2::{KeyboardDriver, PS2KeyboardDriver, get_keyboard},
    multitasking::MANAGER,
    println,
};

// TODO: Move this and the other stuffs such as
// mapper, executer, etc into the OS struct
static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();
pub static KEYBOARD_QUEUE: OnceCell<Mutex<VecDeque<u8>>> = OnceCell::uninit();
static WAKER: AtomicWaker = AtomicWaker::new();

pub(crate) fn add_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if let Err(_) = queue.push(scancode) {
            println!("Scancode queue full");
        } else {
            // wake up the registered waker
            WAKER.wake();
            match MANAGER.try_lock() {
                Some(mut manager) => manager.wake_keyboard(),
                None => {}
            }
        }
    } else {
        println!("Scancode queue have not been initilized");
    }
}

pub struct ScancodeStream {
    _private: (),
}

impl ScancodeStream {
    pub fn new() -> Self {
        SCANCODE_QUEUE
            .try_init_once(|| ArrayQueue::new(128))
            .expect("Dont call this twice");

        Self { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        let queue = SCANCODE_QUEUE.try_get().expect("Uninitialized");

        if let Some(scancode) = queue.pop() {
            // Skips registering a waker if there already is a scancode avalible
            return Poll::Ready(Some(scancode));
        } else {
            // registers a waker to wakeup when a value is avalible
            // the queue might have the code again after registering the waker
            WAKER.register(&cx.waker());
            match queue.pop() {
                Some(value) => {
                    // remove the waker because its nologner needed, we already have the value
                    WAKER.take();
                    Poll::Ready(Some(value))
                }
                None => Poll::Pending,
            }
        }
    }
}

pub async fn process_keypresses() {
    let mut scancodes = ScancodeStream::new();

    // loop through scancodes infinitely
    while let Some(scancode) = scancodes.next().await {
        let test_key_event = get_keyboard().add_byte(scancode);

        if let Ok(Some(key_event)) = test_key_event {
            let thing = get_keyboard().process_keyevent(key_event);
            if let Some(key) = thing {
                PS2KeyboardDriver::handle_key(key);
            }
        }
    }
}
