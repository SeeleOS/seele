#![allow(clippy::module_inception)]

use crate::memory::utils::{Mut, MutGuard};
use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    smp::{current_thread, set_current_thread, with_current_cpu},
    thread::{manager::ThreadManager, thread::Thread},
};

pub mod clone;
pub mod extended_state;
pub mod manager;
pub mod misc;
pub mod scheduling;
pub mod snapshot;
pub mod stack;
pub mod switch;
pub mod thread;
pub mod yielding;

pub static THREAD_MANAGER: OnceCell<Mut<ThreadManager>> = OnceCell::uninit();

pub fn init() {
    without_interrupts(|| {
        THREAD_MANAGER
            .get_or_init(|| Mut::new(ThreadManager::default()))
            .lock()
            .init();
    });
    let scheduler_thread = with_current_cpu(|cpu| cpu.scheduler_thread.clone());
    set_current_thread(Some(scheduler_thread));
}

pub fn with_thread_manager<R>(f: impl FnOnce(&mut ThreadManager) -> R) -> R {
    without_interrupts(|| {
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        f(&mut manager)
    })
}

pub fn try_with_thread_manager<R>(f: impl FnOnce(&mut ThreadManager) -> R) -> Option<R> {
    without_interrupts(|| {
        let mut manager = THREAD_MANAGER.get()?.lock();
        Some(f(&mut manager))
    })
}

pub fn lock_thread_manager() -> MutGuard<'static, ThreadManager> {
    THREAD_MANAGER.get().unwrap().lock()
}

pub type ThreadRef = Arc<Mut<Thread>>;

pub fn get_current_thread() -> ThreadRef {
    current_thread()
}

pub fn scheduler_thread() -> ThreadRef {
    with_current_cpu(|cpu| cpu.scheduler_thread.clone())
}
