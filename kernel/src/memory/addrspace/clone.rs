use core::ptr::copy_nonoverlapping;

use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB, Translate, mapper::TranslateResult,
    },
};

use crate::memory::{
    page_table_wrapper::PageTableWrapped,
    paging::FRAME_ALLOCATOR,
    utils::{apply_offset, as_cow_flags},
};

use super::AddrSpace;

const KERNEL_MEM_START: u64 = 0xffff_8000_0000_0000;

impl AddrSpace {
    pub fn clone_all(&mut self) -> Self {
        log::debug!("addrspace fork");
        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
        log::debug!("frame allocator locked");

        let mut new_page_table = PageTableWrapped::new_with_frame_allocator(&mut frame_allocator);
        let old_page_table = &mut self.page_table;

        for area in self.memory_areas.clone() {
            if area.pages() == 0 {
                continue;
            }

            let start = Page::<Size4KiB>::containing_address(area.start);
            let end = Page::<Size4KiB>::containing_address(area.end - 1u64);
            let pages = Page::<Size4KiB>::range_inclusive(start, end);

            // Loops through all of the mapped pages of the previous addr space
            for page in pages {
                // Get the frame of the page on the old page table
                if let Ok(frame) = old_page_table.inner.translate_page(page)
                    && page.start_address() < VirtAddr::new(KERNEL_MEM_START)
                        // Get the flags
                    && let TranslateResult::Mapped { flags, .. } =
                        old_page_table.inner.translate(page.start_address())
                {
                    if flags.contains(PageTableFlags::WRITABLE) {
                        unsafe {
                            old_page_table
                                .inner
                                .update_flags(page, as_cow_flags(flags))
                                .unwrap()
                                .flush();
                            // Maps the new page with the frame of the old page
                            new_page_table
                                .inner
                                .map_to(page, frame, as_cow_flags(flags), &mut *frame_allocator)
                                .unwrap()
                                .flush()
                        };
                    } else {
                        unsafe {
                            new_page_table
                                .inner
                                .map_to(page, frame, flags, &mut *frame_allocator)
                                .unwrap()
                                .flush();
                        }
                    }
                }
            }
        }

        Self {
            page_table: new_page_table,
            memory_areas: self.memory_areas.clone(),
            user_mem: self.user_mem,
        }
    }
}
