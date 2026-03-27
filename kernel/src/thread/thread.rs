use alloc::{sync::Arc, vec::Vec};
use seele_sys::signal::Signals;
use spin::Mutex;

use crate::{
    process::{Process, ProcessRef},
    thread::{
        ThreadRef,
        misc::{SnapshotState, State, ThreadID},
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
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
        let mut parent_lock = parent.lock();
        let (_, stack) = parent_lock.addrspace.allocate_user(64);
        let kernel_stack_top = parent_lock
            .addrspace
            .allocate_kernel(16)
            .1
            .finish()
            .as_u64();
        Self {
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
        Self {
            snapshot,
            parent,
            kernel_stack_top,
            ..Default::default()
        }
    }
}
