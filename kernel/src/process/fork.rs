use alloc::sync::Arc;
use spin::mutex::Mutex;

use crate::{
    process::{Process, ProcessRef, misc::ProcessID},
    thread::{THREAD_MANAGER, ThreadRef},
};

impl Process {
    pub fn fork(parent: ProcessRef) -> (ProcessRef, ThreadRef) {
        let (pid, new_process) = {
            let current_thread = THREAD_MANAGER
                .get()
                .unwrap()
                .lock()
                .current
                .clone()
                .unwrap();
            let mut parent_locked = parent.lock();
            log::debug!(
                "Forking. Parent Current RSP: {:x}",
                current_thread.lock().snapshot.inner.rsp
            );
            let pid = ProcessID::new();

            log::debug!("fork: parent {} -> child {}", parent_locked.pid.0, pid.0);
            let new_process = Arc::new(Mutex::new(Self {
                pid,
                pending_signals: parent_locked.pending_signals,
                addrspace: parent_locked.addrspace.clone_all(),
                kernel_stack_top: parent_locked.kernel_stack_top,
                objects: parent_locked.objects.clone(),
                current_directory: parent_locked.current_directory.clone(),
                parent: Some(parent.clone()),
                signal_actions: parent_locked.signal_actions.clone(),
                group_id: parent_locked.group_id,
                program_break: parent_locked.program_break,
                file_mode_creation_mask: parent_locked.file_mode_creation_mask,
                ..Default::default()
            }));
            (pid, new_process)
        };

        let current_thread = THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .current
            .clone()
            .unwrap();
        let new_thread = current_thread.lock().clone_and_spawn(new_process.clone());
        new_thread.lock().snapshot.inner.rax = 0;
        new_process.lock().threads.push(Arc::downgrade(&new_thread));

        let _ = pid;
        (new_process, new_thread)
    }
}
