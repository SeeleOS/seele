use core::ops::Index;

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
        thread::{
            ThreadRef,
            future::ThreadFuture,
            misc::{State, ThreadID},
            thread::Thread,
            yielding::BlockedQueues,
        },
    },
    println,
};

#[derive(Default, Debug)]
pub struct ThreadManager {
    pub threads: BTreeMap<ThreadID, ThreadRef>,
    pub current: Option<ThreadRef>,
    pub idle_thread: Option<ThreadRef>,
    pub zombies: Vec<ThreadRef>,
    pub blocked_queues: BlockedQueues,
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

    pub fn kill_all_except(&mut self, thread: ThreadRef) {
        let threads = self
            .current
            .clone()
            .unwrap()
            .lock()
            .parent
            .lock()
            .threads
            .clone();

        let zombies = threads
            .iter()
            .filter(|p| p.upgrade().unwrap().lock().id != thread.lock().id);

        for zombie in zombies {
            self.mark_as_zombie(zombie.upgrade().unwrap());
        }
    }

    pub fn mark_current_as_zombie(&mut self) {
        self.mark_as_zombie(self.current.clone().unwrap());
    }

    pub fn mark_as_zombie(&mut self, thread: ThreadRef) {
        thread.lock().state = State::Zombie;
        self.zombies.push(thread);
    }

    pub fn clean_zombies(&mut self) {
        let mut to_remove = Vec::new();

        println!("zombies size {}", self.zombies.len());

        for ele in self.zombies.drain(..) {
            let parent_arc;
            let thread_id;
            {
                println!("a");
                let thread = ele.lock();
                println!("b");
                parent_arc = thread.parent.clone();
                self.threads.remove(&thread.id);
                thread_id = thread.id;

                drop(thread);
            }
            let mut parent = parent_arc.lock();

            parent
                .threads
                .retain(|t| t.upgrade().is_some_and(|f| f.lock().id != thread_id));
            println!("{:?}", parent.threads);

            if parent.threads.is_empty() {
                to_remove.push(parent_arc.clone());
            }
        }

        for dead_process in to_remove {
            MANAGER.lock().remove_process(dead_process, self);
        }
        println!("clean zombies done");
    }
}
