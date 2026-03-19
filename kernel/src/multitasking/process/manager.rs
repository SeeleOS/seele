use core::any::Any;

use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    vec::Vec,
};
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    filesystem::{path::Path, vfs::VirtualFS},
    multitasking::{
        MANAGER,
        process::{Process, ProcessRef, misc::ProcessID},
        thread::manager::ThreadManager,
    },
    println,
};

#[derive(Debug, Default)]
pub struct Manager {
    pub processes: BTreeMap<ProcessID, ProcessRef>,
    pub current: Option<ProcessRef>,
    pub zombies: Vec<ProcessRef>,
}

impl Manager {
    pub fn init(&mut self) {
        without_interrupts(|| {
            let kernel_process = Process::empty();
            // TODO: delete the idle proecss or let it fucking work with all that shit
            self.current = Some(kernel_process.clone());
            self.processes
                .insert(kernel_process.lock().pid, kernel_process.clone());

            let init = Process::init();
            self.processes.insert(init.lock().pid, init.clone());
        });
    }

    pub fn wake_process_exit(&mut self, process: ProcessRef, thread_manager: &mut ThreadManager) {
        log::debug!("wake process exit {}", process.lock().pid.0);
        thread_manager.wake_process_exit(process.lock().pid);
    }

    pub fn remove_process(&mut self, process: ProcessRef) {
        log::debug!("remove process {}", process.lock().pid.0);
        self.processes.remove(&process.lock().pid);
    }

    pub fn load_process(&mut self, process: ProcessRef) {
        let mut process_locked = process.lock();

        process_locked.addrspace.load();
        self.current = Some(process.clone());
    }
}

pub fn get_current_process() -> ProcessRef {
    MANAGER.lock().current.clone().unwrap()
}
