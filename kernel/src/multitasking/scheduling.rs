use core::panic;

use x86_64::{VirtAddr, instructions::interrupts::without_interrupts};

use crate::{
    misc::snapshot::Snapshot,
    multitasking::{
        MANAGER,
        process::{manager::Manager, process::State},
        thread::{THREAD_MANAGER, manager::ThreadManager, snapshot::ThreadSnapshot},
    },
    s_println,
    tss::TSS,
};

impl ThreadManager {
    fn run_next_unwrapped(&mut self) -> (*mut ThreadSnapshot, *mut ThreadSnapshot) {
        let (current_ptr, current_pid) = {
            let mut current_thread = self.current.as_ref().unwrap().lock();
            let pid = current_thread.parent.lock().pid;

            if current_thread.state == State::Running {
                current_thread.state = State::Ready;
                self.queue.push_back(self.current.clone().unwrap());
            }

            (current_thread.snapshot.as_ptr(), pid)
        }; // Lock released.

        let next_thread_arc = self
            .queue
            .pop_front()
            .unwrap_or(self.idle_thread.clone().unwrap());
        let mut next_thread = next_thread_arc.lock();
        let next_pid = {
            let p = next_thread.parent.lock();
            p.pid
        };
        let next_thread_ptr = next_thread.snapshot.as_ptr();

        if current_pid != next_pid {
            MANAGER.lock().load_process(next_thread.parent.clone());
        }

        next_thread.state = State::Running;
        self.current = Some(next_thread_arc.clone());
        unsafe {
            TSS.privilege_stack_table[0] = VirtAddr::new(next_thread.kernel_stack_top);
        }

        (current_ptr, next_thread_ptr)
    }

    /// picks the next process. called from a zombie process
    fn run_next_zombie_unwrapped(&mut self) -> *mut ThreadSnapshot {
        let next_thread_arc = self
            .queue
            .pop_front()
            .unwrap_or(self.idle_thread.clone().unwrap());
        let mut next_thread = next_thread_arc.lock();
        let next_pid = {
            let p = next_thread.parent.lock();
            p.pid
        };
        let next_thread_ptr = next_thread.snapshot.as_ptr();

        MANAGER.lock().load_process(next_thread.parent.clone());

        next_thread.state = State::Running;
        self.current = Some(next_thread_arc.clone());
        unsafe {
            TSS.privilege_stack_table[0] = VirtAddr::new(next_thread.kernel_stack_top);
        }

        self.clean_zombies();

        next_thread_ptr
    }
}

pub fn run_next(snapshot: &mut Snapshot) {
    let (current, next) = {
        without_interrupts(|| {
            let mut manager = THREAD_MANAGER.get().unwrap().lock();
            manager.run_next_unwrapped()
        })
    };

    unsafe { (*next).switch_from(Some(current.as_mut().unwrap()), Some(snapshot)) };
}

/// runs the next process. called from a zombie process
pub fn run_next_zombie() {
    let next = without_interrupts(|| {
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        manager.run_next_zombie_unwrapped()
    });

    s_println!("next task: {:?}", next);

    unsafe {
        (*next).switch_from(None, None);
    }
}
