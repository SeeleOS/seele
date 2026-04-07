use seele_sys::signal::Signals;

use crate::{
    process::ProcessRef,
    thread::{
        THREAD_MANAGER, ThreadRef,
        misc::{SnapshotState, State, ThreadID},
        snapshot::ThreadSnapshot,
        thread::Thread,
    },
};

impl Thread {
    pub fn clone_and_spawn(&self, process: ProcessRef) -> ThreadRef {
        log::debug!("clone_and_spawn: start");
        let id = ThreadID::default();
        let mut snapshot = self.snapshot;
        let thread = Self {
            parent: process.clone(),
            id,
            snapshot: {
                snapshot.kernel_rsp = process
                    .lock()
                    .addrspace
                    .allocate_kernel(16)
                    .1
                    .finish()
                    .as_u64();
                snapshot
            },
            kernel_stack_top: process
                .lock()
                .addrspace
                .allocate_kernel(16)
                .1
                .finish()
                .as_u64(),
            ..Default::default()
        };

        log::debug!("clone_and_spawn: thread manager lock start");
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        log::debug!("clone_and_spawn: thread manager locked");

        manager.spawn(thread)
    }
}
