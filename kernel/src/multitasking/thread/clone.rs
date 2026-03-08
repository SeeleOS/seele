use core::fmt::Arguments;

use alloc::sync::Arc;
use spin::mutex::Mutex;

use crate::{
    multitasking::{
        kernel_task::{TASK_SPAWNER, task::Task},
        process::ProcessRef,
        thread::{
            THREAD_MANAGER, ThreadRef,
            future::ThreadFuture,
            misc::{State, ThreadID},
            snapshot::ThreadSnapshot,
            thread::Thread,
        },
    },
    s_println,
};

impl Thread {
    pub fn clone_and_spawn(&self, process: ProcessRef) -> ThreadRef {
        s_println!("inside thread fork");
        let id = ThreadID::default();
        let thread = Self {
            parent: process.clone(),
            id,
            snapshot: self.snapshot,
            executor_snapshot: ThreadSnapshot::new_executor(),
            state: State::Ready,
            kernel_stack_top: process
                .lock()
                .addrspace
                .allocate_kernel(16)
                .1
                .finish()
                .as_u64(),
        };

        s_println!("thredad mgr lock start");
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        s_println!("fd");

        manager.spawn(thread)
    }
}
