use x86_64::{VirtAddr, instructions::interrupts::without_interrupts};

use crate::{
    misc::snapshot::Snapshot,
    multitasking::{
        MANAGER,
        thread::{THREAD_MANAGER, manager::ThreadManager, misc::State, snapshot::ThreadSnapshot},
    },
    s_println,
    tss::TSS,
};

pub fn return_to_executor(snapshot: &mut Snapshot) {
    let (thread_snapshot, executor_snapshot) = {
        let manager = THREAD_MANAGER.get().unwrap().lock();
        let current_ref = manager.current.clone().unwrap();
        let mut current = current_ref.lock();

        (
            &mut current.snapshot as *mut ThreadSnapshot,
            &mut current.executor_snapshot as *mut ThreadSnapshot,
        )
    };

    unsafe { (*executor_snapshot).switch_from(Some(&mut *thread_snapshot), Some(snapshot)) };
}

pub fn return_to_executor_from_current() {
    return_to_executor(&mut Snapshot::from_current());
}

pub fn return_to_executor_no_save() {
    let (thread_snapshot, executor_snapshot) = {
        let manager = THREAD_MANAGER.get().unwrap().lock();
        let current_ref = manager.current.clone().unwrap();
        let mut current = current_ref.lock();

        (
            &mut current.snapshot as *mut ThreadSnapshot,
            &mut current.executor_snapshot as *mut ThreadSnapshot,
        )
    };

    unsafe { (*executor_snapshot).switch_from(Some(&mut *thread_snapshot), None) };
}
