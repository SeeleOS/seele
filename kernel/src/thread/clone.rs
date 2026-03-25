use crate::{
    process::ProcessRef,
    thread::{
        THREAD_MANAGER, ThreadRef,
        misc::{State, ThreadID},
        snapshot::ThreadSnapshot,
        thread::Thread,
    },
};

impl Thread {
    pub fn clone_and_spawn(&self, process: ProcessRef) -> ThreadRef {
        log::debug!("clone_and_spawn: start");
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

        log::debug!("clone_and_spawn: thread manager lock start");
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        log::debug!("clone_and_spawn: thread manager locked");

        manager.spawn(thread)
    }
}
