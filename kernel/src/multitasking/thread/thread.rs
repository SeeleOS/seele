use alloc::sync::Arc;
use spin::Mutex;
use x86_64::{VirtAddr, structures::paging::OffsetPageTable};

use crate::{
    misc::snapshot::Snapshot,
    multitasking::{
        MANAGER,
        memory::{allocate_kernel_stack, allocate_stack},
        process::{ProcessRef, process::Process},
        thread::{
            ThreadRef,
            misc::{State, ThreadID},
            snapshot::{ThreadSnapshot, ThreadSnapshotType},
        },
    },
    s_println,
};

#[derive(Debug)]
pub struct Thread {
    pub parent: ProcessRef,
    pub id: ThreadID,
    pub snapshot: ThreadSnapshot,
    pub executor_snapshot: ThreadSnapshot,
    pub state: State,
    pub kernel_stack_top: u64,
}

impl Thread {
    pub fn empty() -> ThreadRef {
        Arc::new(Mutex::new(Thread {
            parent: Process::empty(),
            id: ThreadID::default(),
            snapshot: ThreadSnapshot::default(),
            executor_snapshot: ThreadSnapshot::new_executor(),
            state: State::Ready,
            kernel_stack_top: 0,
        }))
    }
}

impl Thread {
    pub fn new(entry_point: u64, parent: ProcessRef) -> Self {
        let stack = allocate_stack(16, &mut parent.lock().page_table.inner);
        let kernel_stack_top = allocate_kernel_stack(16, &mut parent.lock().page_table.inner)
            .finish()
            .as_u64();
        Self {
            snapshot: ThreadSnapshot::new(
                entry_point,
                &mut parent.clone().lock().page_table,
                stack.finish().as_u64(),
                ThreadSnapshotType::Thread,
            ),
            executor_snapshot: ThreadSnapshot::new_executor(),
            parent,
            kernel_stack_top,
            state: State::Ready,
            id: ThreadID::default(),
        }
    }

    pub fn from_snapshot(
        snapshot: ThreadSnapshot,
        parent: ProcessRef,
        kernel_stack_top: u64,
    ) -> Self {
        Self {
            snapshot,
            executor_snapshot: ThreadSnapshot::new_executor(),
            parent,
            state: State::Ready,
            id: ThreadID::default(),
            kernel_stack_top,
        }
    }
}
