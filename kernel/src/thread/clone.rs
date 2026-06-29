use crate::{
    process::ProcessRef,
    thread::{
        ThreadRef, misc::ThreadID, stack::allocate_owned_kernel_stack, thread::Thread,
        with_thread_manager, yielding::BlockType,
    },
};

impl Thread {
    fn clone_for_spawn_with_id(&self, process: ProcessRef, id: ThreadID) -> Thread {
        log::debug!("clone_and_spawn: start");
        let mut snapshot = self.snapshot.clone();
        let kernel_stack = allocate_owned_kernel_stack(16).finish();
        let kernel_stack_top = kernel_stack.top().as_u64();
        let scheduler_stack = allocate_owned_kernel_stack(16).finish();
        let scheduler_stack_top = scheduler_stack.top().as_u64();
        snapshot.kernel_rsp = kernel_stack_top;
        let mount_namespace_snapshot = process.lock().mount_namespace_snapshot.clone();
        let mut thread = Self::new_base(crate::thread::thread::ThreadInit {
            parent: process,
            id,
            snapshot,
            scheduler_snapshot: crate::thread::snapshot::ThreadSnapshot::new_scheduler(
                scheduler_stack_top,
            ),
            kernel_stack: Some(kernel_stack),
            scheduler_stack: Some(scheduler_stack),
            kernel_stack_top,
            mount_namespace_snapshot,
        });
        thread.blocked_signals = self.blocked_signals;
        thread.saved_blocked_signals = self.saved_blocked_signals.clone();
        thread.last_syscall_no = self.last_syscall_no;
        thread.last_user_snapshot = self.last_user_snapshot;
        thread.last_user_fs_base = self.last_user_fs_base;
        thread.name = self.name;
        thread
    }

    pub fn clone_and_spawn(&self, process: ProcessRef) -> ThreadRef {
        self.clone_and_spawn_with_id(process, ThreadID::new())
    }

    pub fn clone_and_spawn_with_id(&self, process: ProcessRef, id: ThreadID) -> ThreadRef {
        let thread = self.clone_for_spawn_with_id(process, id);

        log::debug!("clone_and_spawn: thread manager lock start");
        with_thread_manager(|manager| {
            log::debug!("clone_and_spawn: thread manager locked");
            manager.spawn(thread)
        })
    }

    pub fn clone_and_spawn_blocked_with_id(
        &self,
        process: ProcessRef,
        id: ThreadID,
        block_type: BlockType,
    ) -> ThreadRef {
        let thread = self.clone_for_spawn_with_id(process, id);

        log::debug!("clone_and_spawn_blocked: thread manager lock start");
        with_thread_manager(|manager| {
            log::debug!("clone_and_spawn_blocked: thread manager locked");
            manager.spawn_blocked(thread, block_type)
        })
    }
}
