use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::mutex::Mutex;

use crate::{
    memory::{addrspace::AddrSpace, page_table_wrapper::PageTableWrapped},
    multitasking::{
        MANAGER,
        process::{Process, misc::ProcessID},
    },
};

impl Process {
    fn fork(&self) {
        let mut new_threads = Vec::new();
        let pid = ProcessID::default();

        for ele in self.threads.clone() {
            new_threads.push(Arc::downgrade(
                &Weak::upgrade(&ele)
                    .unwrap()
                    .lock()
                    .clone_and_spawn(MANAGER.lock().current.clone().unwrap().clone()),
            ));
        }

        let new_process = Arc::new(Mutex::new(Self {
            pid,
            addrspace: self.addrspace.fork(),
            kernel_stack_top: self.kernel_stack_top,
            threads: new_threads,
            objects: self.objects.clone(),
            current_directory: self.current_directory.clone(),
        }));

        MANAGER.lock().processes.insert(pid, new_process);
    }
}
