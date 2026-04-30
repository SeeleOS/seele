use crate::{
    process::ProcessRef,
    thread::{
        THREAD_MANAGER, ThreadRef, misc::ThreadID, stack::allocate_kernel_stack, thread::Thread,
        yielding::BlockType,
    },
};

impl Thread {
    fn clone_for_spawn_with_id(&self, process: ProcessRef, id: ThreadID) -> Thread {
        log::debug!("clone_and_spawn: start");
        let mut snapshot = self.snapshot;
        Self {
            parent: process.clone(),
            id,
            snapshot: {
                snapshot.kernel_rsp = allocate_kernel_stack(16).finish().as_u64();
                snapshot
            },
            kernel_stack_top: allocate_kernel_stack(16).finish().as_u64(),
            blocked_signals: self.blocked_signals,
            saved_blocked_signals: self.saved_blocked_signals.clone(),
            last_syscall_no: self.last_syscall_no,
            last_user_snapshot: self.last_user_snapshot,
            last_user_fs_base: self.last_user_fs_base,
            name: self.name,
            ..Default::default()
        }
    }

    pub fn clone_and_spawn(&self, process: ProcessRef) -> ThreadRef {
        self.clone_and_spawn_with_id(process, ThreadID::new())
    }

    pub fn clone_and_spawn_with_id(&self, process: ProcessRef, id: ThreadID) -> ThreadRef {
        let thread = self.clone_for_spawn_with_id(process, id);

        log::debug!("clone_and_spawn: thread manager lock start");
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        log::debug!("clone_and_spawn: thread manager locked");

        manager.spawn(thread)
    }

    pub fn clone_and_spawn_blocked_with_id(
        &self,
        process: ProcessRef,
        id: ThreadID,
        block_type: BlockType,
    ) -> ThreadRef {
        let thread = self.clone_for_spawn_with_id(process, id);

        log::debug!("clone_and_spawn_blocked: thread manager lock start");
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        log::debug!("clone_and_spawn_blocked: thread manager locked");

        manager.spawn_blocked(thread, block_type)
    }
}
