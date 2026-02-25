use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB},
};

use crate::memory::{
        PHYSICAL_MEMORY_OFFSET,
        paging::FRAME_ALLOCATOR,
        utils::{apply_offset, copy_kernel_mapping},
    };

#[derive(Debug)]
pub struct PageTableWrapped {
    pub frame: PhysFrame<Size4KiB>,
    pub inner: OffsetPageTable<'static>,
}

impl Default for PageTableWrapped {
    fn default() -> Self {
        // allocates a frame for the l4 page table to be stored at
        let page_table_frame = FRAME_ALLOCATOR
            .get()
            .unwrap()
            .lock()
            .allocate_frame()
            .expect("No more space");

        let table_addr = VirtAddr::new(apply_offset(page_table_frame.start_address().as_u64()));

        // Get it as a page table
        let page_table: &mut PageTable = unsafe { &mut *(table_addr.as_mut_ptr()) };

        page_table.zero();

        copy_kernel_mapping(page_table);

        Self {
            frame: page_table_frame,
            inner: unsafe {
                OffsetPageTable::new(
                    page_table,
                    VirtAddr::new(*PHYSICAL_MEMORY_OFFSET.get().unwrap()),
                )
            },
        }
    }
}
