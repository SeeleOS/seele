use core::arch::naked_asm;

use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;

use crate::{
    multitasking::{
        MANAGER,
        kernel_task::{TASK_SPAWNER, task::Task},
        process::misc::State,
        thread::{self, ThreadRef, future::ThreadFuture, misc::ThreadID, thread::Thread},
    },
    s_println,
};

#[derive(Default, Debug)]
pub struct ThreadManager {
    pub threads: BTreeMap<ThreadID, ThreadRef>,
    pub current: Option<ThreadRef>,
    pub queue: VecDeque<ThreadRef>,
    pub idle_thread: Option<ThreadRef>,
    pub zombies: Vec<ThreadRef>,
}

impl ThreadManager {
    pub fn init(&mut self) {
        self.current = Some(Thread::empty());
    }

    pub fn spawn(&mut self, thread: Thread) -> ThreadRef {
        let id = thread.id;
        let thread = Arc::new(Mutex::new(thread));

        self.threads.insert(id, thread);

        let thread = self.threads.get_mut(&id).unwrap();

        TASK_SPAWNER
            .get()
            .unwrap()
            .lock()
            .spawn(Task::new(ThreadFuture(thread.clone())));

        thread.clone()
    }

    pub fn mark_current_as_zombie(&mut self) {
        self.mark_as_zombie(self.current.clone().unwrap());
    }

    pub fn mark_as_zombie(&mut self, thread: ThreadRef) {
        thread.lock().state = State::Zombie;
        self.zombies.push(thread);
    }

    pub fn clean_zombies(&mut self) {
        for ele in self.zombies.drain(..) {
            let thread = ele.lock();
            let parent_arc = thread.parent.clone();
            let parent = parent_arc.lock();
            self.threads.remove(&thread.id);

            if parent.threads.is_empty() {
                MANAGER.lock().remove_process(parent_arc.clone());
            }
        }
    }
}
