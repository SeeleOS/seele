use seele_sys::signal::Signals;

use crate::{
    process::ProcessRef,
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
        log::debug!("clone_and_spawn: start");
        let id = ThreadID::new();
        let mut snapshot = self.snapshot;
        let thread = Self {
            parent: process.clone(),
            id,
            snapshot: {
                snapshot.kernel_rsp = allocate_kernel_stack(16).finish().as_u64();
                snapshot
            },
            kernel_stack_top: allocate_kernel_stack(16).finish().as_u64(),
            ..Default::default()
        };

        log::debug!("clone_and_spawn: thread manager lock start");
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        log::debug!("clone_and_spawn: thread manager locked");

        manager.spawn(thread)
    }
}
