use core::ptr::copy_nonoverlapping;

use acpi::registers;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB, Translate,
};

use crate::memory::{
    addrspace::AddrSpace, page_table_wrapper::PageTableWrapped, paging::FRAME_ALLOCATOR,
    utils::apply_offset,
};

impl AddrSpace {
    /// Clone all the memory thats in [`self`] to [`target`]
    pub fn fork(&self) -> Self {
        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();

        let mut new_page_table = PageTableWrapped::default();
        let old_page_table = &self.page_table;

        for region in self.used_memories.clone() {
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(region.start),
                Page::containing_address(region.end),
            );

            for page in pages {
                if let Some(addr) = old_page_table.inner.translate_addr(page.start_address()) {
                    let old_addr = apply_offset(addr.as_u64());
                    let frame = frame_allocator.allocate_frame().unwrap();
                    let new_addr = apply_offset(frame.start_address().as_u64());

                    unsafe {
                        new_page_table
                            .inner
                            .map_to(
                                page,
                                frame,
                                PageTableFlags::USER_ACCESSIBLE
                                    | PageTableFlags::WRITABLE
                                    | PageTableFlags::PRESENT,
                                &mut *frame_allocator,
                            )
                            .unwrap()
                            .flush()
                    };

                    unsafe {
                        copy_nonoverlapping(old_addr as *const u8, new_addr as *mut u8, 4096)
                    };
                }
            }
        }

        Self {
            page_table: new_page_table,
            used_memories: self.used_memories.clone(),
            user_mem: self.user_mem,
            kernel_mem: self.kernel_mem,
        }
    }
}
