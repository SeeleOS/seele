use alloc::sync::{Arc, Weak};
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::multitasking::thread::{manager::ThreadManager, thread::Thread};

pub mod future;
pub mod manager;
pub mod misc;
pub mod snapshot;
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
