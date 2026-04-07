use crate::{
    gdt::GDT, memory::addrspace::AddrSpace, misc::snapshot::Snapshot,
    thread::stack::allocate_kernel_stack,
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadSnapshot {
    pub inner: Snapshot,
    // RSP used on context switching in kernel space to not messup the userstack
    pub kernel_rsp: u64,
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
    // Snapshot for the poll()
    // function of the thread
    Executor,
}

impl ThreadSnapshot {
    pub fn new(
        entry_point: u64,
        addrspace: &mut AddrSpace,
        virt_stack_addr: u64,
        snapshot_type: ThreadSnapshotType,
    ) -> Self {
        log::trace!(
            "ThreadSnapshot::new: entry_point = {:#x}, user_rsp = {:#x}",
            entry_point,
            virt_stack_addr
        );
        Self {
            inner: Snapshot::default_regs(
                entry_point,
                GDT.1.user_code.0,
                0x202,
                virt_stack_addr,
                GDT.1.user_data.0,
            ),
            kernel_rsp: allocate_kernel_stack(16).finish().as_u64(),
            snapshot_type,
            ..Default::default()
        }
    }

    pub fn new_executor() -> Self {
        Self {
            snapshot_type: ThreadSnapshotType::Executor,
            kernel_rsp: allocate_kernel_stack(16).finish().as_u64(),
            ..Default::default()
        }
    }

    pub fn as_ptr(&mut self) -> *mut Self {
        self as *mut Self
    }
}
