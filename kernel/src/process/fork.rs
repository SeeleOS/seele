use alloc::{sync::Arc, vec::Vec};
use seele_sys::signal::Signals;
use spin::{MutexGuard, mutex::Mutex};

use crate::{
    process::{Process, manager::Manager, misc::ProcessID},
    thread::THREAD_MANAGER,
};

impl Process {
    pub fn fork(&mut self, manager: &mut MutexGuard<Manager>) -> ProcessID {
        log::debug!("inside fork");
        let pid = ProcessID::default();
        let current_thread = THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .current
            .clone()
            .unwrap();
        log::debug!(
            "Forking. Parent Current RSP: {:x}",
            current_thread.lock().snapshot.inner.rsp
        );

        log::debug!("fork: parent {} -> child {}", self.pid.0, pid.0);
        let parent = manager.current.clone().unwrap();
        let new_process = Arc::new(Mutex::new(Self {
            pid,
            pending_signals: self.pending_signals,
            addrspace: self.addrspace.clone_all(),
            kernel_stack_top: self.kernel_stack_top,
            threads: Vec::new(),
            objects: self.objects.clone(),
            current_directory: self.current_directory.clone(),
            exit_code: None,
            parent: Some(parent),
            signal_actions: self.signal_actions.clone(),
            blocked_signals: Signals::default(),
        }));

        let new_thread = current_thread.lock().clone_and_spawn(new_process.clone());
        new_thread.lock().snapshot.inner.rax = 0;
        new_process.lock().threads.push(Arc::downgrade(&new_thread));

        manager.processes.insert(pid, new_process);

        pid
    }
}
