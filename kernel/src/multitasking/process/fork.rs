use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::{MutexGuard, mutex::Mutex};

use crate::{
    memory::{addrspace::AddrSpace, page_table_wrapper::PageTableWrapped},
    multitasking::{
        MANAGER,
        process::{Process, ProcessRef, manager::Manager, misc::ProcessID},
        thread::THREAD_MANAGER,
    },
};

impl Process {
    pub fn fork(&self, manager: &mut MutexGuard<Manager>) -> ProcessID {
        log::info!("inside fork");
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

        let new_process = Arc::new(Mutex::new(Self {
            pid,
            addrspace: self.addrspace.clone_all(),
            kernel_stack_top: self.kernel_stack_top,
            threads: Vec::new(),
            objects: self.objects.clone(),
            current_directory: self.current_directory.clone(),
            exit_code: None,
        }));

        let new_thread = current_thread.lock().clone_and_spawn(new_process.clone());
        new_thread.lock().snapshot.inner.rax = 0;
        new_process.lock().threads.push(Arc::downgrade(&new_thread));

        manager.processes.insert(pid, new_process);

        pid
    }
}
