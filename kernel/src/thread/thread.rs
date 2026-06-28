use crate::memory::utils::Mut;
use alloc::{sync::Arc, vec::Vec};

use crate::{
    misc::snapshot::Snapshot,
    process::{Process, ProcessRef},
    signal::{PendingSignalInfo, SIGNAL_AMOUNT, Signals},
    thread::{
        ThreadRef,
        misc::{BlockedSyscall, SnapshotState, State, ThreadID},
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
        stack::{KernelStack, allocate_owned_kernel_stack},
    },
};

pub const DEFAULT_USER_TIMESLICE_NS: u64 = 4_000_000;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LinuxStack {
    pub ss_sp: u64,
    pub ss_flags: i32,
    pub ss_size: usize,
}

impl Default for LinuxStack {
    fn default() -> Self {
        Self {
            ss_sp: 0,
            ss_flags: 2,
            ss_size: 0,
        }
    }
}

#[derive(Debug)]
pub struct Thread {
    pub parent: ProcessRef,
    pub id: ThreadID,
    pub snapshot_state: SnapshotState,
    pub snapshot: ThreadSnapshot,
    pub scheduler_snapshot: ThreadSnapshot,
    pub state: State,
    // Kernel stack for the cpu to switch to a clean stack on interrupts
    // not to be confused with the kernel_rsp in ThreadSnapshot
    pub kernel_stack_top: u64,
    pub kernel_stack: Option<KernelStack>,
    pub scheduler_stack: Option<KernelStack>,

    pub saved_blocked_signals: Vec<Signals>,
    pub blocked_signals: Signals,
    pub temporary_blocked_signals: Option<(Signals, Signals)>,
    pub pending_signals: Signals,
    pub pending_signal_info: Vec<Option<PendingSignalInfo>>,
    pub interrupted_by_signal: bool,
    pub interruptible_wait_active: bool,
    pub clear_child_tid: u64,
    pub robust_list_head: u64,
    pub robust_list_len: usize,
    pub rseq_area: u64,
    pub rseq_len: u32,
    pub rseq_flags: u32,
    pub rseq_sig: u32,
    pub last_syscall_no: u64,
    pub last_user_snapshot: Snapshot,
    pub last_user_fs_base: u64,
    pub active_syscall_profile: Option<BlockedSyscall>,
    pub blocked_syscall_started_at: Option<u64>,
    pub blocked_syscall_cycles: u64,
    pub io_buffer: Vec<u8>,
    pub name: [u8; 16],
    pub timeslice_remaining_ns: u64,
    pub sigaltstack: LinuxStack,
    pub mount_namespace_snapshot: Option<Vec<u64>>,

    pub sig_handler_snapshot: ThreadSnapshot,
}

impl Default for Thread {
    fn default() -> Self {
        Self {
            saved_blocked_signals: Vec::new(),
            mount_namespace_snapshot: None,
            sig_handler_snapshot: ThreadSnapshot::default(),
            snapshot_state: SnapshotState::default(),
            parent: Process::empty(),
            id: ThreadID::default(),
            snapshot: ThreadSnapshot::default(),
            scheduler_snapshot: ThreadSnapshot::default(),
            state: State::Ready,
            kernel_stack_top: 0,
            kernel_stack: None,
            scheduler_stack: None,
            blocked_signals: Signals::default(),
            temporary_blocked_signals: None,
            pending_signals: Signals::default(),
            pending_signal_info: alloc::vec![None; SIGNAL_AMOUNT],
            interrupted_by_signal: false,
            interruptible_wait_active: false,
            clear_child_tid: 0,
            robust_list_head: 0,
            robust_list_len: 0,
            rseq_area: 0,
            rseq_len: 0,
            rseq_flags: 0,
            rseq_sig: 0,
            last_syscall_no: 0,
            last_user_snapshot: Snapshot::default(),
            last_user_fs_base: 0,
            active_syscall_profile: None,
            blocked_syscall_started_at: None,
            blocked_syscall_cycles: 0,
            io_buffer: Vec::new(),
            name: [0; 16],
            timeslice_remaining_ns: DEFAULT_USER_TIMESLICE_NS,
            sigaltstack: LinuxStack::default(),
        }
    }
}

impl Thread {
    pub fn empty() -> ThreadRef {
        let kernel_stack = allocate_owned_kernel_stack(16).finish();
        let kernel_stack_top = kernel_stack.top().as_u64();
        let scheduler_stack = allocate_owned_kernel_stack(16).finish();
        let scheduler_stack_top = scheduler_stack.top().as_u64();

        Arc::new(Mut::new(Thread {
            kernel_stack_top,
            kernel_stack: Some(kernel_stack),
            scheduler_snapshot: ThreadSnapshot::new_scheduler(scheduler_stack_top),
            scheduler_stack: Some(scheduler_stack),
            ..Default::default()
        }))
    }
}

impl Thread {
    pub fn new(entry_point: u64, parent: ProcessRef) -> Self {
        Self::new_with_id(entry_point, parent, ThreadID::new())
    }

    pub fn new_with_id(entry_point: u64, parent: ProcessRef, id: ThreadID) -> Self {
        let mut parent_lock = parent.lock();
        let (_, stack) = parent_lock.addrspace.allocate_user_stack(64);
        let user_stack = stack.finish().as_u64();
        let kernel_stack = allocate_owned_kernel_stack(16).finish();
        let kernel_stack_top = kernel_stack.top().as_u64();
        let scheduler_stack = allocate_owned_kernel_stack(16).finish();
        let scheduler_stack_top = scheduler_stack.top().as_u64();
        Self {
            id,
            snapshot: ThreadSnapshot::new(
                entry_point,
                &mut parent.clone().lock().addrspace,
                user_stack,
                kernel_stack_top,
                ThreadSnapshotType::Thread,
            ),
            last_user_snapshot: Snapshot::default_regs(
                entry_point,
                crate::smp::user_code_selector().0,
                0x202,
                user_stack,
                crate::smp::user_data_selector().0,
            ),
            parent: parent.clone(),
            kernel_stack_top,
            kernel_stack: Some(kernel_stack),
            scheduler_snapshot: ThreadSnapshot::new_scheduler(scheduler_stack_top),
            scheduler_stack: Some(scheduler_stack),
            mount_namespace_snapshot: parent_lock.mount_namespace_snapshot.clone(),
            ..Default::default()
        }
    }

    pub fn from_snapshot(
        snapshot: ThreadSnapshot,
        parent: ProcessRef,
        kernel_stack: KernelStack,
    ) -> Self {
        Self::from_snapshot_with_id(snapshot, parent, kernel_stack, ThreadID::new())
    }

    pub fn from_snapshot_with_id(
        snapshot: ThreadSnapshot,
        parent: ProcessRef,
        kernel_stack: KernelStack,
        id: ThreadID,
    ) -> Self {
        let kernel_stack_top = kernel_stack.top().as_u64();
        let scheduler_stack = allocate_owned_kernel_stack(16).finish();
        let scheduler_stack_top = scheduler_stack.top().as_u64();
        Self {
            id,
            last_user_snapshot: snapshot.inner,
            last_user_fs_base: snapshot.fs_base,
            snapshot,
            parent,
            kernel_stack_top,
            kernel_stack: Some(kernel_stack),
            scheduler_snapshot: ThreadSnapshot::new_scheduler(scheduler_stack_top),
            scheduler_stack: Some(scheduler_stack),
            ..Default::default()
        }
    }
}
