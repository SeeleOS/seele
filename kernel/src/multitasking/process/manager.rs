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
            process::{self, Process, ProcessID},
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
            let idle_process = Process::empty();

            self.current = Some(kernel_process.clone());
            self.processes
                .insert(kernel_process.lock().pid, kernel_process.clone());

            // TODO: remove these test processes
            self.spawn(&ELF_HOLDER.data);
        });
    }

    pub fn spawn(&mut self, program: &[u8]) {
        let process = Process::new(program);
        let pid = process.lock().pid;
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

    pub fn block_current_unwrappped(&mut self, block_type: BlockType) {
        let current = self.current.clone().unwrap();

        current.lock().state = process::State::Blocked(block_type);
        //self.queue.into_iter().filter(|p| *p != current.pid.clone());

        match block_type {
            BlockType::WakeRequired(wake_type) => match wake_type {
                WakeType::Keyboard => self.blocked_queues.keyboard.push_back(current),
                WakeType::IO => self.blocked_queues.io.push_back(current),
            },
            _ => {}
        }

        //run_next();
    }
}

pub fn block_current(block_type: BlockType) {
    MANAGER.lock().block_current_unwrappped(block_type);
    // TODO
    //run_next(InterruptStackFrame::new(fwefwefas, code_segment, cpu_flags, stack_pointer, stack_segment));
}
