use alloc::{collections::btree_map::BTreeMap, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    process::manager::MANAGER,
    systemcall::implementations::wake_futex_for_process,
    task::{TASK_SPAWNER, task::Task},
    thread::{
        ThreadRef,
        future::ThreadFuture,
        misc::{State, ThreadID},
        thread::Thread,
        yielding::BlockedQueues,
    },
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

        log::debug!("thread spawn: {:?}", id);
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
            self.mark_thread_exited(zombie.upgrade().unwrap());
        }
    }

    pub fn mark_current_thread_exited(&mut self) {
        log::debug!("mark_current_thread_exited");
        self.mark_thread_exited(self.current.clone().unwrap());
    }

    pub fn mark_thread_exited(&mut self, thread: ThreadRef) {
        log::debug!("mark_thread_exited");
        {
            let mut thread = thread.lock();
            log::debug!("mark_thread_exited tid={:?}", thread.id);
            if thread.clear_child_tid != 0 {
                crate::s_println!(
                    "thread exit clear_child_tid: pid={} tid={} addr={:#x}",
                    thread.parent.lock().pid.0,
                    thread.id.0,
                    thread.clear_child_tid
                );
                unsafe {
                    *(thread.clear_child_tid as *mut i32) = 0;
                }
                let pid = thread.parent.lock().pid.0;
                wake_futex_for_process(pid, thread.clear_child_tid, 1);
                thread.clear_child_tid = 0;
            }
            thread.state = State::Zombie;
        }
        self.zombies.push(thread);
    }

    pub fn cleanup_exited_threads(&mut self) {
        let mut to_remove = Vec::new();

        log::debug!("zombies size {}", self.zombies.len());

        for ele in self.zombies.drain(..) {
            let parent_arc;
            let thread_id;
            {
                log::trace!("clean_zombies: lock thread");
                let thread = ele.lock();
                log::trace!("clean_zombies: locked thread");
                parent_arc = thread.parent.clone();
                self.threads.remove(&thread.id);
                thread_id = thread.id;

                drop(thread);
            }
            let mut parent = parent_arc.lock();

            parent
                .threads
                .retain(|t| t.upgrade().is_some_and(|f| f.lock().id != thread_id));
            log::trace!("clean_zombies: remaining threads {:?}", parent.threads);

            if parent.threads.is_empty() {
                to_remove.push(parent_arc.clone());
            }
        }

        for dead_process in to_remove {
            MANAGER
                .lock()
                .notify_process_exit_waiters(dead_process, self);
        }
        log::debug!("cleanup_exited_threads done");
    }
}
