#![allow(clippy::module_inception)]

use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::{
    smp::{current_thread, set_current_thread},
    thread::{manager::ThreadManager, thread::Thread},
};

pub mod clone;
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
    let idle_thread = Thread::empty();
    thread_manager.init(idle_thread.clone());
    set_current_thread(Some(idle_thread));
}

pub type ThreadRef = Arc<Mutex<Thread>>;

pub fn get_current_thread() -> ThreadRef {
    current_thread()
}

pub fn scheduler_thread() -> ThreadRef {
    THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .idle_thread
        .clone()
        .expect("scheduler thread not initialized")
}
