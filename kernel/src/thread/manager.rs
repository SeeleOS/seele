use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::Arc,
    vec::Vec,
};
use core::mem;
use spin::Mutex;

use crate::{
    object::linux_anon::wake_signalfd_for_process_with_manager,
    process::{ProcessRef, manager::MANAGER},
    signal::{Signal, Signals},
    smp::current_thread,
    systemcall::implementations::wake_futex_for_process_with_manager,
    thread::{
        ThreadRef,
        misc::{State, ThreadID},
        thread::Thread,
        yielding::{BlockType, BlockedQueues},
    },
};

#[derive(Debug)]
struct PendingThreadExit {
    process: ProcessRef,
    clear_child_tid: u64,
}

#[derive(Default, Debug)]
pub struct ThreadManager {
    pub threads: BTreeMap<ThreadID, ThreadRef>,
    pub idle_thread: Option<ThreadRef>,
    pub ready_queue: VecDeque<ThreadRef>,
    pub zombies: Vec<ThreadRef>,
    pending_thread_exits: Vec<PendingThreadExit>,
    pub blocked_queues: BlockedQueues,
}

impl ThreadManager {
    pub fn init(&mut self, idle_thread: ThreadRef) {
        self.idle_thread = Some(idle_thread);
    }

    pub fn spawn(&mut self, thread: Thread) -> ThreadRef {
        let id = thread.id;
        let thread = Arc::new(Mutex::new(thread));

        self.threads.insert(id, thread.clone());

        log::debug!("thread spawn: {:?}", id);
        self.push_ready(thread.clone());

        thread
    }

    pub fn push_ready(&mut self, thread: ThreadRef) {
        if self
            .ready_queue
            .iter()
            .any(|queued| Arc::ptr_eq(queued, &thread))
        {
            return;
        }

        self.ready_queue.push_back(thread);
    }

    pub fn pop_ready(&mut self) -> Option<ThreadRef> {
        while let Some(thread) = self.ready_queue.pop_front() {
            if matches!(thread.lock().state, State::Ready) {
                return Some(thread);
            }
        }

        None
    }

    pub fn has_ready_threads(&self) -> bool {
        !self.ready_queue.is_empty()
    }

    pub fn kill_all_except(&mut self, thread: ThreadRef) {
        let threads = self
            .idle_thread
            .clone()
            .unwrap_or_else(current_thread)
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
        self.mark_thread_exited(crate::thread::get_current_thread());
    }

    pub fn mark_thread_exited(&mut self, thread: ThreadRef) {
        log::debug!("mark_thread_exited");
        let (process, clear_child_tid) = {
            let mut thread = thread.lock();
            log::debug!("mark_thread_exited tid={:?}", thread.id);
            let process = thread.parent.clone();
            let clear_child_tid = thread.clear_child_tid;

            if clear_child_tid != 0 {
                thread.clear_child_tid = 0;
            }

            (process, clear_child_tid)
        };

        self.remove_from_blocked_queues(&thread);
        thread.lock().state = State::Zombie;

        if clear_child_tid != 0 {
            self.pending_thread_exits.push(PendingThreadExit {
                process,
                clear_child_tid,
            });
        }

        self.zombies.push(thread);
    }

    pub fn cleanup_exited_threads(&mut self) {
        let mut to_remove = Vec::new();

        self.flush_pending_thread_exits();

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
            if let Some(parent) = dead_process.lock().parent.clone() {
                let (parent_pid, threads) = {
                    let mut parent = parent.lock();
                    parent
                        .pending_signals
                        .insert(Signals::from(Signal::ChildChanged));
                    (parent.pid.0, parent.threads.clone())
                };

                wake_signalfd_for_process_with_manager(parent_pid, self);

                for thread in threads {
                    let Some(thread) = thread.upgrade() else {
                        continue;
                    };

                    let should_wake = {
                        let thread = thread.lock();
                        matches!(
                            &thread.state,
                            State::Blocked(block_type) if !matches!(block_type, BlockType::Stopped)
                        )
                    };

                    if should_wake {
                        self.wake(thread.clone());
                    }
                }
            }
            MANAGER
                .lock()
                .notify_process_exit_waiters(dead_process, self);
        }
        log::debug!("cleanup_exited_threads done");
    }

    fn flush_pending_thread_exits(&mut self) {
        for pending in mem::take(&mut self.pending_thread_exits) {
            let pid = {
                let mut process = pending.process.lock();
                let pid = process.pid.0;
                let _ = process
                    .addrspace
                    .write(pending.clear_child_tid as *mut u8, &0i32);
                pid
            };
            wake_futex_for_process_with_manager(pid, pending.clear_child_tid, 1, self);
        }
    }
}
