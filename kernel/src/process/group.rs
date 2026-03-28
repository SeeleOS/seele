use core::sync::atomic::AtomicU64;

use alloc::vec::Vec;
use seele_sys::signal::Signal;

use crate::process::{ProcessRef, manager::Manager};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProcessGroupID(pub u64);

impl Default for ProcessGroupID {
    fn default() -> Self {
        static NEXT_GID: AtomicU64 = AtomicU64::new(0);

        Self(NEXT_GID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
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
