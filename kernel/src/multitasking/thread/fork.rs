use core::fmt::Arguments;

use alloc::sync::Arc;
use spin::mutex::Mutex;

use crate::multitasking::{
    kernel_task::{TASK_SPAWNER, task::Task},
    process::ProcessRef,
    thread::{
        THREAD_MANAGER, ThreadRef,
        future::ThreadFuture,
        misc::{State, ThreadID},
        snapshot::ThreadSnapshot,
        thread::Thread,
    },
};

impl Thread {
    pub fn fork(&self, process: ProcessRef) -> ThreadRef {
        let id = ThreadID::default();
        let thread = Arc::new(Mutex::new(Self {
            parent: process.clone(),
            id,
            snapshot: self.snapshot,
            executor_snapshot: ThreadSnapshot::new_executor(),
            state: State::Ready,
            kernel_stack_top: self.kernel_stack_top,
        }));

        let mut manager = THREAD_MANAGER.get().unwrap().lock();

        manager.threads.insert(id, thread.clone());

        TASK_SPAWNER
            .get()
            .unwrap()
            .lock()
            .spawn(Task::new(ThreadFuture(thread.clone())));

        thread
    }
}
