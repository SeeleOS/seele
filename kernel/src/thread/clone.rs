use crate::{
    process::ProcessRef,
    signal::Signals,
    thread::{
        THREAD_MANAGER, ThreadRef,
        misc::{SnapshotState, State, ThreadID},
        snapshot::ThreadSnapshot,
        stack::allocate_kernel_stack,
        thread::Thread,
    },
};

impl Thread {
    pub fn clone_and_spawn(&self, process: ProcessRef) -> ThreadRef {
        self.clone_and_spawn_with_id(process, ThreadID::new())
    }

    pub fn clone_and_spawn_with_id(&self, process: ProcessRef, id: ThreadID) -> ThreadRef {
        log::debug!("clone_and_spawn: start");
        let mut snapshot = self.snapshot;
        let thread = Self {
            parent: process.clone(),
            id,
            snapshot: {
                snapshot.kernel_rsp = allocate_kernel_stack(16).finish().as_u64();
                snapshot
            },
            kernel_stack_top: allocate_kernel_stack(16).finish().as_u64(),
            blocked_signals: self.blocked_signals,
            saved_blocked_signals: self.saved_blocked_signals.clone(),
            ..Default::default()
        };

        log::debug!("clone_and_spawn: thread manager lock start");
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        log::debug!("clone_and_spawn: thread manager locked");

        manager.spawn(thread)
    }
}
