use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    vec::Vec,
};
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    misc::{self, hlt_loop},
    multitasking::{
        MANAGER,
        process::{
            ProcessRef,
            misc::ProcessID,
            process::{self, Process},
        },
        yielding::{BlockType, BlockedQueues, WakeType},
    },
    print, println, s_println,
};

#[derive(Debug, Default)]
pub struct Manager {
    pub processes: BTreeMap<ProcessID, ProcessRef>,
    pub current: Option<ProcessRef>,
    pub queue: VecDeque<ProcessRef>,
    pub zombies: Vec<ProcessRef>,
    pub blocked_queues: BlockedQueues,
}

#[repr(align(8))]
struct AlignedElf {
    data: [u8; include_bytes!("../../../../libc-test/test.elf").len()],
}

static ELF_HOLDER: AlignedElf = AlignedElf {
    data: *include_bytes!("../../../../libc-test/test.elf"),
};

impl Manager {
    pub fn init(&mut self) {
        without_interrupts(|| {
            let kernel_process = Process::empty();
            // TODO: delete the idle proecss or let it fucking work with all that shit
            self.current = Some(kernel_process.clone());
            self.processes
                .insert(kernel_process.lock().pid, kernel_process.clone());

            self.spawn(&ELF_HOLDER.data);
        });
    }

    pub fn spawn(&mut self, program: &[u8]) {
        let process = Process::new(program);
        self.processes.insert(process.lock().pid, process.clone());
        self.queue.push_back(process.clone());
    }

    pub fn remove_process(&mut self, process: ProcessRef) {
        self.processes.remove(&process.lock().pid);
    }

    pub fn load_process(&mut self, process: ProcessRef) {
        let mut process_locked = process.lock();

        process_locked.page_table.load();
        self.current = Some(process.clone());
    }
}
