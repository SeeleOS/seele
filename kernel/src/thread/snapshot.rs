use crate::{
    memory::addrspace::AddrSpace,
    misc::snapshot::Snapshot,
    smp::{user_code_selector, user_data_selector},
    thread::extended_state::ExtendedState,
};

#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct ThreadSnapshot {
    pub inner: Snapshot,
    // RSP used on context switching in kernel space to not messup the userstack
    pub kernel_rsp: u64,
    pub extended_state: ExtendedState,
    pub kernel_extended_state: ExtendedState,
    pub fs_base: u64,
    pub snapshot_type: ThreadSnapshotType,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadSnapshotType {
    // Snapshot of the thread it self
    #[default]
    Thread,
    // Snapshot of a blocked kernel context (e.g. syscall blocking)
    Kernel,
    // Snapshot for the scheduler context of the thread.
    Scheduler,
}

impl ThreadSnapshot {
    pub fn new(
        entry_point: u64,
        addrspace: &mut AddrSpace,
        virt_stack_addr: u64,
        kernel_stack_top: u64,
        snapshot_type: ThreadSnapshotType,
    ) -> Self {
        Self::new_with_extended_state(
            entry_point,
            addrspace,
            virt_stack_addr,
            kernel_stack_top,
            snapshot_type,
            ExtendedState::capture_current(),
        )
    }

    pub fn new_with_extended_state(
        entry_point: u64,
        _addrspace: &mut AddrSpace,
        virt_stack_addr: u64,
        kernel_stack_top: u64,
        snapshot_type: ThreadSnapshotType,
        extended_state: ExtendedState,
    ) -> Self {
        log::trace!(
            "ThreadSnapshot::new: entry_point = {:#x}, user_rsp = {:#x}",
            entry_point,
            virt_stack_addr
        );
        Self {
            inner: Snapshot::default_regs(
                entry_point,
                user_code_selector().0,
                0x202,
                virt_stack_addr,
                user_data_selector().0,
            ),
            kernel_rsp: kernel_stack_top,
            extended_state,
            kernel_extended_state: ExtendedState::capture_current(),
            fs_base: 0,
            snapshot_type,
        }
    }

    pub fn new_scheduler(kernel_stack_top: u64) -> Self {
        Self {
            inner: Snapshot::default(),
            snapshot_type: ThreadSnapshotType::Scheduler,
            kernel_rsp: kernel_stack_top,
            extended_state: ExtendedState::capture_current(),
            kernel_extended_state: ExtendedState::capture_current(),
            fs_base: 0,
        }
    }

    pub fn as_ptr(&mut self) -> *mut Self {
        self as *mut Self
    }
}
