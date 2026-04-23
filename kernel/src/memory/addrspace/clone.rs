use x86_64::{
    VirtAddr,
    structures::paging::{
        Mapper, Page, PageTableFlags, Size4KiB, Translate, mapper::TranslateResult,
    },
};

use crate::memory::{
    addrspace::cow::{increase_ref, increase_ref_by, is_ref_counted},
    page_table_wrapper::PageTableWrapped,
    paging::FRAME_ALLOCATOR,
    utils::as_cow_flags,
};

use super::AddrSpace;

const KERNEL_MEM_START: u64 = 0xffff_9000_0000_0000;

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
                    unsafe {
                        let already_ref_counted = is_ref_counted(frame);
                        let new_flags = if flags.contains(PageTableFlags::WRITABLE) {
                            old_page_table
                                .inner
                                .update_flags(page, as_cow_flags(flags))
                                .unwrap()
                                .flush();
                            as_cow_flags(flags)
                        } else {
                            flags
                        };

                        // Only writable pages should enter the CoW path. Read-only
                        // mappings can stay shared with their original permissions.
                        new_page_table
                            .inner
                            .map_to(page, frame, new_flags, &mut *frame_allocator)
                            .unwrap()
                            .flush();
                        if already_ref_counted {
                            increase_ref(frame);
                        } else {
                            // This frame was private before the fork. After cloning,
                            // both parent and child own one mapping, so the initial
                            // tracked refcount must start at 2 rather than 1.
                            increase_ref_by(frame, 2);
                        }
                    }
                }
            }
        }

        Self {
            page_table: new_page_table,
            memory_areas: self.memory_areas.clone(),
            user_mem: self.user_mem,
            last_area_index: None,
        }
    }
}
