use core::task::Poll;

use alloc::collections::vec_deque::VecDeque;
use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use futures_util::{Stream, StreamExt, task::AtomicWaker};
use pc_keyboard::DecodedKey;

use crate::{
    keyboard::{
        ps2::_PS2_KEYBOARD,
        scancode_stream::{SCANCODE_QUEUE, WAKER},
    },
    multitasking::{MANAGER, thread::THREAD_MANAGER},
    println,
};

pub mod block_device;
pub mod decoding_task;
pub mod object;
pub mod ps2;
pub mod scancode_stream;

pub(crate) fn push_scancode(scancode: u8) {
    let queue = SCANCODE_QUEUE.get().unwrap();
    queue.push(scancode).unwrap();

    // wake up the registered waker
    WAKER.wake();
    // Wakeup all the blocked process (it should be threads now lol)
    THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
}
