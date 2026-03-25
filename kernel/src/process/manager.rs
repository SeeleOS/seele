use alloc::{collections::btree_map::BTreeMap, vec::Vec};
use lazy_static::lazy_static;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    process::{Process, ProcessRef, misc::ProcessID},
    thread::{THREAD_MANAGER, manager::ThreadManager},
};

lazy_static! {
    pub static ref MANAGER: spin::Mutex<Manager> = spin::Mutex::new(Manager::default());
}

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

    pub fn notify_process_exit_waiters(
        &mut self,
        process: ProcessRef,
        thread_manager: &mut ThreadManager,
    ) {
        log::debug!("notify process exit waiters {}", process.lock().pid.0);
        thread_manager.wake_process_exit_waiters(process.lock().pid);
    }

    pub fn terminate_process(&mut self, process: ProcessRef) {
        for thread in &process.lock().threads {
            THREAD_MANAGER
                .get()
                .unwrap()
                .lock()
                .mark_thread_exited(thread.upgrade().unwrap());
        }
    }

    pub fn destroy_process(&mut self, process: ProcessRef) {
        log::debug!("destroy process {}", process.lock().pid.0);
        self.processes.remove(&process.lock().pid);

        process.lock().addrspace.clean();
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
