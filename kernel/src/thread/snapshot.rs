use crate::{
    memory::addrspace::AddrSpace,
    misc::snapshot::Snapshot,
    smp::{user_code_selector, user_data_selector},
    thread::stack::allocate_kernel_stack,
};

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
pub struct FxState {
    bytes: [u8; 512],
}

impl FxState {
    pub fn capture_current() -> Self {
        let mut state = Self { bytes: [0; 512] };

        unsafe {
            core::arch::asm!(
                "fxsave64 [{ptr}]",
                ptr = in(reg) state.bytes.as_mut_ptr(),
                options(nostack)
            );
        }

        state
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.bytes.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.bytes.as_mut_ptr()
    }
}

impl Default for FxState {
    fn default() -> Self {
        Self::capture_current()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadSnapshot {
    pub inner: Snapshot,
    // RSP used on context switching in kernel space to not messup the userstack
    pub kernel_rsp: u64,
    pub fx_state: FxState,
    pub fs_base: u64,
    pub snapshot_type: ThreadSnapshotType,
}

#[derive(Default, Clone, Copy, Debug)]
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
        snapshot_type: ThreadSnapshotType,
    ) -> Self {
        Self::new_with_fx_state(
            entry_point,
            addrspace,
            virt_stack_addr,
            snapshot_type,
            FxState::capture_current(),
        )
    }

    pub fn new_with_fx_state(
        entry_point: u64,
        _addrspace: &mut AddrSpace,
        virt_stack_addr: u64,
        snapshot_type: ThreadSnapshotType,
        fx_state: FxState,
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
            kernel_rsp: allocate_kernel_stack(16).finish().as_u64(),
            fx_state,
            fs_base: 0,
            snapshot_type,
        }
    }

    pub fn new_scheduler() -> Self {
        Self {
            inner: Snapshot::default(),
            snapshot_type: ThreadSnapshotType::Scheduler,
            kernel_rsp: allocate_kernel_stack(16).finish().as_u64(),
            fx_state: FxState::capture_current(),
            fs_base: 0,
        }
    }

    pub fn as_ptr(&mut self) -> *mut Self {
        self as *mut Self
    }
}
