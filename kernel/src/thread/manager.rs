use crate::memory::utils::Mut;
use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::{Arc, Weak},
    vec::Vec,
};
use core::mem;

use crate::{
    object::linux_anon::{
        wake_pidfd_for_process_with_manager, wake_signalfd_for_process_with_manager,
    },
    process::ProcessRef,
    signal::Signals,
    systemcall::implementations::wake_futex_for_process_with_manager,
    thread::{
        ThreadRef,
        misc::{State, ThreadID},
        scheduling::request_all_cpus_resched,
        snapshot::ThreadSnapshotType,
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
        let cpu_count = if crate::SMP_ENABLED {
            crate::smp::topology::processors().len()
        } else {
            1
        };
        self.resize_ready_queues(cpu_count);
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
        let thread = Arc::new(Mut::new(thread));

        self.threads.insert(id, thread.clone());

        log::debug!("thread spawn: {:?}", id);
        self.push_ready_balanced(thread.clone());

        thread
    }

    pub fn spawn_blocked(&mut self, thread: Thread, block_type: BlockType) -> ThreadRef {
        let id = thread.id;
        let thread = Arc::new(Mut::new(thread));

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
        self.pop_ready_for_cpu_inner(cpu_index, true, ReadyThreadFilter::Any)
    }

    pub fn pop_user_ready_for_cpu(&mut self, cpu_index: usize) -> Option<ThreadRef> {
        self.pop_ready_for_cpu_inner(cpu_index, true, ReadyThreadFilter::UserThread)
    }

    pub fn pop_local_ready_for_cpu(&mut self, cpu_index: usize) -> Option<ThreadRef> {
        self.pop_ready_for_cpu_inner(cpu_index, false, ReadyThreadFilter::Any)
    }

    fn pop_ready_for_cpu_inner(
        &mut self,
        cpu_index: usize,
        allow_steal: bool,
        filter: ReadyThreadFilter,
    ) -> Option<ThreadRef> {
        if self.ready_queues.is_empty() {
            self.resize_ready_queues(1);
        }

        let local_index = cpu_index.min(self.ready_queues.len() - 1);
        if let Some(thread) =
            Self::pop_ready_from_queue_matching(&mut self.ready_queues[local_index], filter)
        {
            return Some(thread);
        }
        if !allow_steal {
            return None;
        }

        for index in 0..self.ready_queues.len() {
            if index == local_index {
                continue;
            }
            if let Some(thread) =
                Self::pop_ready_from_queue_matching(&mut self.ready_queues[index], filter)
            {
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
        self.exit_process_threads_except(thread);
    }

    pub fn exit_process_threads_except(&mut self, current: ThreadRef) -> bool {
        let (threads, current_id) = {
            let current = current.lock();
            (current.parent.lock().threads.clone(), current.id)
        };
        self.exit_thread_list_except(threads, current_id)
    }

    pub fn exit_thread_list_except(
        &mut self,
        threads: Vec<Weak<Mut<Thread>>>,
        current_id: ThreadID,
    ) -> bool {
        let mut all_stopped = true;

        for weak in threads {
            let Some(thread) = weak.upgrade() else {
                continue;
            };
            let mut thread_lock = thread.lock();
            if thread_lock.id == current_id {
                continue;
            }

            match thread_lock.state {
                State::Zombie => {}
                State::Running => {
                    thread_lock.state = State::Exiting;
                    request_all_cpus_resched();
                    all_stopped = false;
                }
                State::Exiting => {
                    all_stopped = false;
                }
                State::Ready | State::Blocking(_) | State::Woken | State::Blocked(_) => {
                    drop(thread_lock);
                    self.mark_thread_exited(thread);
                }
            }
        }

        all_stopped
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

    pub fn cleanup_exited_threads(&mut self) -> Vec<ProcessRef> {
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

        for dead_process in &to_remove {
            let (parent, child_exit_signal) = {
                let dead_process = dead_process.lock();
                (dead_process.parent.clone(), dead_process.child_exit_signal)
            };
            if let Some(parent) = parent {
                let (parent_pid, threads) = {
                    let mut parent = parent.lock();
                    parent
                        .pending_signals
                        .insert(Signals::from(child_exit_signal));
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
            let pid = dead_process.lock().pid;
            self.wake_process_exit_waiters(pid);
            wake_pidfd_for_process_with_manager(pid.0, self);
        }
        log::debug!("cleanup_exited_threads done");
        to_remove
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

    pub fn remove_ready_thread(&mut self, thread: &ThreadRef) -> bool {
        let mut removed = false;
        for queue in &mut self.ready_queues {
            let old_len = queue.len();
            queue.retain(|queued| !Arc::ptr_eq(queued, thread));
            removed |= queue.len() != old_len;
        }
        removed
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

    fn pop_ready_from_queue_matching(
        queue: &mut VecDeque<ThreadRef>,
        filter: ReadyThreadFilter,
    ) -> Option<ThreadRef> {
        let queued_len = queue.len();
        for _ in 0..queued_len {
            let Some(thread) = queue.pop_front() else {
                break;
            };

            let is_eligible = {
                let mut thread_lock = thread.lock();
                matches!(thread_lock.state, State::Ready)
                    && filter.matches(thread_lock.get_appropriate_snapshot().snapshot_type)
            };
            if is_eligible {
                return Some(thread);
            }

            if matches!(thread.lock().state, State::Ready) {
                queue.push_back(thread);
            }
        }

        None
    }
}

#[derive(Clone, Copy)]
enum ReadyThreadFilter {
    Any,
    UserThread,
}

impl ReadyThreadFilter {
    fn matches(self, snapshot_type: ThreadSnapshotType) -> bool {
        match self {
            Self::Any => true,
            Self::UserThread => matches!(snapshot_type, ThreadSnapshotType::Thread),
        }
    }
}
