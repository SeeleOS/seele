#![allow(clippy::module_inception)]

use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::thread::{manager::ThreadManager, thread::Thread};

pub mod clone;
pub mod future;
pub mod manager;
pub mod misc;
pub mod scheduling;
pub mod snapshot;
pub mod stack;
pub mod switch;
pub mod thread;
pub mod yielding;

pub static THREAD_MANAGER: OnceCell<Mutex<ThreadManager>> = OnceCell::uninit();

pub fn init() {
    let mut thread_manager = THREAD_MANAGER
        .get_or_init(|| Mutex::new(ThreadManager::default()))
        .lock();
    thread_manager.init();
}

pub type ThreadRef = Arc<Mutex<Thread>>;

pub fn get_current_thread() -> ThreadRef {
    THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .current
        .clone()
        .unwrap()
}
