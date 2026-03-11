use core::ptr::copy_nonoverlapping;

use acpi::registers;
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB, Translate},
};

use crate::{
    memory::{
        addrspace::AddrSpace, page_table_wrapper::PageTableWrapped, paging::FRAME_ALLOCATOR,
        utils::apply_offset,
    },
};

const KERNEL_MEM_START: u64 = 0xffff_8000_0000_0000;

impl AddrSpace {
    pub fn clone_all(&self) -> Self {
        log::debug!("addrspace fork");
        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
        log::debug!("frame allocator locked");

        let mut new_page_table = PageTableWrapped::new_with_frame_allocator(&mut frame_allocator);
        let old_page_table = &self.page_table;

        for region in self.used_memories.clone() {
            let pages = Page::<Size4KiB>::range(
                Page::containing_address(region.start),
                Page::containing_address(region.end),
            );

            for page in pages {
                if let Some(addr) = old_page_table.inner.translate_addr(page.start_address())
                    && page.start_address() < VirtAddr::new(KERNEL_MEM_START)
                {
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
        }
    }
}
