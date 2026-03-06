use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    vec::Vec,
};
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    filesystem::{path::Path, vfs::VirtualFS},
    multitasking::process::{Process, ProcessRef, misc::ProcessID},
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

            self.spawn(Path::new("/programs/mash.elf"));
        });
    }

    pub fn spawn(&mut self, program: Path) {
        let mut vfs = VirtualFS.lock();
        let size = vfs.file_info(program.clone()).unwrap().size;

        let mut buf = alloc::vec![0u8; size];
        vfs.read_file(program, &mut buf).unwrap();

        let process = Process::new(&buf);
        self.processes.insert(process.lock().pid, process.clone());
    }

    pub fn remove_process(&mut self, process: ProcessRef) {
        self.processes.remove(&process.lock().pid);
    }

    pub fn load_process(&mut self, process: ProcessRef) {
        let mut process_locked = process.lock();

        process_locked.addrspace.load();
        self.current = Some(process.clone());
    }
}
