use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    vec::Vec,
};
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    misc::hlt_loop,
    multitasking::{
        MANAGER,
        process::process::{self, Process, ProcessID},
        yielding::{BlockType, BlockedQueues, WakeType},
    },
    print, println,
};

#[derive(Debug, Default)]
pub struct Manager {
    pub processes: BTreeMap<ProcessID, Process>,
    pub current: Option<ProcessID>,
    pub queue: VecDeque<ProcessID>,
    pub zombies: Vec<ProcessID>,
    pub blocked_queues: BlockedQueues,

    pub idle_process: Option<ProcessID>,
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
            let kernel_process = Process::default();
            let idle_process = Process::new(&ELF_HOLDER.data);

            self.current = Some(kernel_process.pid);
            self.processes.insert(kernel_process.pid, kernel_process);

            self.idle_process = Some(idle_process.pid);
            self.processes.insert(idle_process.pid, idle_process);

            // TODO: remove these test processes
            self.spawn(&ELF_HOLDER.data);
        });
    }

    pub fn spawn(&mut self, program: &[u8]) {
        let process = Process::new(program);
        let pid = process.pid;
        self.processes.insert(pid, process);
        self.queue.push_back(pid);
    }

    pub fn clean_zombies(&mut self) {
        for zombie in self.zombies.drain(..) {
            self.processes.remove(&zombie);
            self.current.take_if(|p| *p == zombie);
        }
    }

    pub fn block_current_unwrappped(&mut self, block_type: BlockType) {
        let current = self.processes.get_mut(&self.current.unwrap()).unwrap();

        current.state = process::State::Blocked(block_type);
        //self.queue.into_iter().filter(|p| *p != current.pid.clone());

        match block_type {
            BlockType::WakeRequired(wake_type) => match wake_type {
                WakeType::Keyboard => self.blocked_queues.keyboard.push_back(current.pid.clone()),
                WakeType::IO => self.blocked_queues.io.push_back(current.pid.clone()),
            },
            _ => {}
        }

        //run_next();
    }

    pub fn get_current(&mut self) -> &mut Process {
        self.processes
            .get_mut(&self.current.unwrap())
            .expect("The current process doesnt exist on process list WTF")
    }
}

pub fn block_current(block_type: BlockType) {
    MANAGER.lock().block_current_unwrappped(block_type);
    // TODO
    //run_next(InterruptStackFrame::new(fwefwefas, code_segment, cpu_flags, stack_pointer, stack_segment));
}

pub extern "C" fn test3() -> ! {
    loop {
        let x = 1;
        print!("{x}");
    }
}

pub extern "C" fn test2() -> ! {
    loop {
        let x = 2;
        print!("{x}");
    }
}

pub extern "C" fn testz() -> ! {
    loop {
        let x = 3;
        print!("{}", x);
    }
}

pub extern "C" fn idle() -> ! {
    println!("idle");
    hlt_loop()
}
