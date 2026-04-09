use alloc::vec::Vec;
use seele_sys::signal::Signal;

use crate::process::{
    ProcessRef,
    manager::{MANAGER, Manager},
};

#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProcessGroupID(pub u64);

impl ProcessGroupID {
    pub fn from_leader(pid: crate::process::misc::ProcessID) -> Self {
        Self(pid.0)
    }
}

impl ProcessGroupID {
    pub fn get_processes(self) -> Vec<ProcessRef> {
        MANAGER.lock().get_processes_in_group(self)
    }
}

impl Manager {
    pub fn get_processes_in_group(&mut self, group_id: ProcessGroupID) -> Vec<ProcessRef> {
        let mut processes = Vec::new();

        for (_, process) in &mut self.processes {
            if process.lock().group_id == group_id {
                processes.push(process.clone());
            }
        }

        processes
    }
}
