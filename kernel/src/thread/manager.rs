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
    pub ready_queues: Vec<VecDeque<ThreadRef>>,
    pub zombies: Vec<ThreadRef>,
    pending_thread_exits: Vec<PendingThreadExit>,
    pub blocked_queues: BlockedQueues,
    next_ready_cpu: usize,
}

impl ThreadManager {
    pub fn init(&mut self) {
        self.resize_ready_queues(crate::smp::topology::processors().len().max(1));
    }

    pub fn resize_ready_queues(&mut self, cpu_count: usize) {
        let cpu_count = cpu_count.max(1);
        if self.ready_queues.len() >= cpu_count {
            return;
        }

        self.ready_queues.resize_with(cpu_count, VecDeque::new);
        if self.next_ready_cpu >= self.ready_queues.len() {
            self.next_ready_cpu = 0;
        }
    }

    pub fn spawn(&mut self, thread: Thread) -> ThreadRef {
        let id = thread.id;
        let thread = Arc::new(Mutex::new(thread));

        self.threads.insert(id, thread.clone());

        log::debug!("thread spawn: {:?}", id);
        self.push_ready_balanced(thread.clone());

        thread
    }

    pub fn spawn_blocked(&mut self, thread: Thread, block_type: BlockType) -> ThreadRef {
        let id = thread.id;
        let thread = Arc::new(Mutex::new(thread));

        self.threads.insert(id, thread.clone());

        log::debug!("thread spawn blocked: {:?} {:?}", id, block_type);
        {
            let mut locked = thread.lock();
            locked.state = State::Blocked(block_type.clone());
        }
        self.blocked_queues.push(thread.clone(), id, block_type);

        thread
    }

    pub fn push_ready_balanced(&mut self, thread: ThreadRef) {
        let cpu_index = self.next_balanced_cpu();
        self.push_ready_on_cpu(thread, cpu_index);
    }

    pub fn push_ready_on_cpu(&mut self, thread: ThreadRef, cpu_index: usize) {
        if self.ready_queues.is_empty() {
            self.resize_ready_queues(1);
        }

        if self.is_ready_queued(&thread) {
            return;
        }

        let queue_index = cpu_index.min(self.ready_queues.len() - 1);
        self.ready_queues[queue_index].push_back(thread);
    }

    pub fn pop_ready_for_cpu(&mut self, cpu_index: usize) -> Option<ThreadRef> {
        self.pop_ready_for_cpu_inner(cpu_index, true)
    }

    pub fn pop_local_ready_for_cpu(&mut self, cpu_index: usize) -> Option<ThreadRef> {
        self.pop_ready_for_cpu_inner(cpu_index, false)
    }

    fn pop_ready_for_cpu_inner(
        &mut self,
        cpu_index: usize,
        allow_steal: bool,
    ) -> Option<ThreadRef> {
        if self.ready_queues.is_empty() {
            self.resize_ready_queues(1);
        }

        let local_index = cpu_index.min(self.ready_queues.len() - 1);
        if let Some(thread) = Self::pop_ready_from_queue(&mut self.ready_queues[local_index]) {
            return Some(thread);
        }
        if !allow_steal {
            return None;
        }

        for index in 0..self.ready_queues.len() {
            if index == local_index {
                continue;
            }
            if let Some(thread) = Self::pop_ready_from_queue(&mut self.ready_queues[index]) {
                return Some(thread);
            }
        }

        None
    }

    pub fn has_ready_threads(&self) -> bool {
        self.ready_queues.iter().any(|queue| !queue.is_empty())
    }

    pub fn has_ready_threads_for_cpu(&self, cpu_index: usize) -> bool {
        self.ready_queues
            .get(cpu_index)
            .is_some_and(|queue| !queue.is_empty())
    }

    pub fn wake_thread_by_id(&mut self, thread_id: ThreadID) {
        if let Some(thread) = self.threads.get(&thread_id).cloned() {
            self.wake(thread);
        }
    }

    pub fn kill_all_except(&mut self, thread: ThreadRef) {
        let threads = self
            .threads
            .get(&thread.lock().id)
            .cloned()
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
                        .insert(Signals::from(Signal::SIGCHLD));
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

    pub fn remove_ready_thread(&mut self, thread: &ThreadRef) {
        for queue in &mut self.ready_queues {
            queue.retain(|queued| !Arc::ptr_eq(queued, thread));
        }
    }

    fn next_balanced_cpu(&mut self) -> usize {
        if self.ready_queues.is_empty() {
            self.resize_ready_queues(1);
        }

        let cpu_index = self.next_ready_cpu % self.ready_queues.len();
        self.next_ready_cpu = (cpu_index + 1) % self.ready_queues.len();
        cpu_index
    }

    fn is_ready_queued(&self, thread: &ThreadRef) -> bool {
        self.ready_queues
            .iter()
            .any(|queue| queue.iter().any(|queued| Arc::ptr_eq(queued, thread)))
    }

    fn pop_ready_from_queue(queue: &mut VecDeque<ThreadRef>) -> Option<ThreadRef> {
        while let Some(thread) = queue.pop_front() {
            if matches!(thread.lock().state, State::Ready) {
                return Some(thread);
            }
        }

        None
    }
}
