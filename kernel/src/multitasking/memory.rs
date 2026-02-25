
use x86_64::structures::paging::{
        OffsetPageTable, PageTableFlags,
    };

use crate::{
    memory::manager::{allocate_kernel_mem, allocate_user_mem},
    misc::stack_builder::StackBuilder,
};

/// Returns the virtual address of the stack top
/// and the offsetted physical address of the stack top
///
/// Note: The phys addr of the stack top is the addr of the
/// last frame, so if you writes more then 4KiB of memory
/// it will cause undefined behaviour
pub fn allocate_stack(pages: u64, table: &mut OffsetPageTable<'static>) -> StackBuilder {
    allocate_user_mem(
        pages,
        table,
        PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE | PageTableFlags::PRESENT,
    )
    .1
}

pub fn allocate_kernel_stack(pages: u64, table: &mut OffsetPageTable<'static>) -> StackBuilder {
    allocate_kernel_mem(
        pages,
        table,
        PageTableFlags::WRITABLE | PageTableFlags::PRESENT,
    )
    .1
}
