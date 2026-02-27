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
    multitasking::MANAGER,
    println,
};

pub mod decoding_task;
pub mod object;
pub mod ps2;
pub mod scancode_stream;

pub(crate) fn push_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if let Err(_) = queue.push(scancode) {
            panic!("Scancode queue full");
        } else {
            // wake up the registered waker
            WAKER.wake();
            // Wakeup all the blocked process (it should be threads now lol)
            match MANAGER.try_lock() {
                Some(mut manager) => manager.wake_keyboard(),
                None => {}
            }
        }
    } else {
        println!("Scancode queue have not been initilized");
    }
}
