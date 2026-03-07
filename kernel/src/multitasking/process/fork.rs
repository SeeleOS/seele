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
    s_println,
};

impl Process {
    pub fn fork(&self, ref_to_self: ProcessRef, mut manager: MutexGuard<Manager>) {
        s_println!("inside fork!");
        let mut new_threads = Vec::new();
        let pid = ProcessID::default();
        let current_thread = THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .current
            .clone()
            .unwrap();

        let new_thread = current_thread.lock().clone_and_spawn(ref_to_self.clone());

        s_println!("woa");
        new_threads.push(Arc::downgrade(&new_thread));
        s_println!("wopa end");

        s_println!("f");
        let new_process = Arc::new(Mutex::new(Self {
            pid,
            addrspace: self.addrspace.clone_all(),
            kernel_stack_top: self.kernel_stack_top,
            threads: new_threads,
            objects: self.objects.clone(),
            current_directory: self.current_directory.clone(),
        }));
        s_println!("da");

        manager.processes.insert(pid, new_process);
    }
}
