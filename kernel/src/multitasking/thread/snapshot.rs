use x86_64::registers::control::Cr3Flags;

use crate::{
    gdt::GDT,
    memory::page_table_wrapper::PageTableWrapped,
    misc::{others::calc_cr3_value, snapshot::Snapshot},
    multitasking::memory::allocate_kernel_stack,
};

// NOTE: the direction of the struct in memory and the stack is REVERSED
// therefore you need to push rbp - r15 and then rflags
// and also, ptr.sub(1) 6 times (rbp-r15) and then write the rflags
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
    // Snapshot for the poll()
    // function of the thread
    Executor,
}

impl ThreadSnapshot {
    pub fn new(
        entry_point: u64,
        table: &mut PageTableWrapped,
        virt_stack_addr: u64,
        snapshot_type: ThreadSnapshotType,
    ) -> Self {
        Self {
            inner: Snapshot::default_regs(
                entry_point,
                GDT.1.user_code.0,
                0x202,
                virt_stack_addr,
                GDT.1.user_data.0,
            ),
            kernel_rsp: allocate_kernel_stack(16, &mut table.inner)
                .finish()
                .as_u64(),
            snapshot_type,
            ..Default::default()
        }
    }

    pub fn new_executor() -> Self {
        Self {
            snapshot_type: ThreadSnapshotType::Executor,
            ..Default::default()
        }
    }

    pub fn as_ptr(&mut self) -> *mut Self {
        self as *mut Self
    }
}
