use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    process::{Process, ProcessRef},
    signal::Signals,
    task::task::TaskID,
    thread::{
        ThreadRef,
        misc::{SnapshotState, State, ThreadID},
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
        stack::allocate_kernel_stack,
    },
};

#[derive(Debug)]
pub struct Thread {
    pub parent: ProcessRef,
    pub id: ThreadID,
    pub snapshot_state: SnapshotState,
    pub snapshot: ThreadSnapshot,
    pub executor_snapshot: ThreadSnapshot,
    pub state: State,
    // Kernel stack for the cpu to switch to a clean stack on interrupts
    // not to be confused with the kernel_rsp in ThreadSnapshot
    pub kernel_stack_top: u64,

    pub saved_blocked_signals: Vec<Signals>,
    pub blocked_signals: Signals,
    pub clear_child_tid: u64,
    pub task_id: Option<TaskID>,
    pub robust_list_head: u64,
    pub robust_list_len: usize,
    pub rseq_area: u64,
    pub rseq_len: u32,
    pub rseq_flags: u32,
    pub rseq_sig: u32,

    pub sig_handler_snapshot: ThreadSnapshot,
}

impl Default for Thread {
    fn default() -> Self {
        Self {
            saved_blocked_signals: Vec::new(),
            sig_handler_snapshot: ThreadSnapshot::default(),
            snapshot_state: SnapshotState::default(),
            parent: Process::empty(),
            id: ThreadID::default(),
            snapshot: ThreadSnapshot::default(),
            executor_snapshot: ThreadSnapshot::new_executor(),
            state: State::Ready,
            kernel_stack_top: 0,
            blocked_signals: Signals::default(),
            clear_child_tid: 0,
            task_id: None,
            robust_list_head: 0,
            robust_list_len: 0,
            rseq_area: 0,
            rseq_len: 0,
            rseq_flags: 0,
            rseq_sig: 0,
        }
    }
}

impl Thread {
    pub fn empty() -> ThreadRef {
        Arc::new(Mutex::new(Thread::default()))
    }
}

impl Thread {
    pub fn new(entry_point: u64, parent: ProcessRef) -> Self {
        Self::new_with_id(entry_point, parent, ThreadID::new())
    }

    pub fn new_with_id(entry_point: u64, parent: ProcessRef, id: ThreadID) -> Self {
        let mut parent_lock = parent.lock();
        let (_, stack) = parent_lock.addrspace.allocate_user(64);
        let kernel_stack_top = allocate_kernel_stack(16).finish().as_u64();
        Self {
            id,
            snapshot: ThreadSnapshot::new(
                entry_point,
                &mut parent.clone().lock().addrspace,
                stack.finish().as_u64(),
                ThreadSnapshotType::Thread,
            ),
            parent: parent.clone(),
            kernel_stack_top,
            ..Default::default()
        }
    }

    pub fn from_snapshot(
        snapshot: ThreadSnapshot,
        parent: ProcessRef,
        kernel_stack_top: u64,
    ) -> Self {
        Self::from_snapshot_with_id(snapshot, parent, kernel_stack_top, ThreadID::new())
    }

    pub fn from_snapshot_with_id(
        snapshot: ThreadSnapshot,
        parent: ProcessRef,
        kernel_stack_top: u64,
        id: ThreadID,
    ) -> Self {
        Self {
            id,
            snapshot,
            parent,
            kernel_stack_top,
            ..Default::default()
        }
    }
}
