use core::intrinsics::copy_nonoverlapping;

use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB, Translate, mapper::TranslateResult,
    },
};

use crate::memory::{
    addrspace::{AddrSpace, clone::COW_FLAG},
    paging::FRAME_ALLOCATOR,
    utils::apply_offset,
};

impl AddrSpace {
    // Replace the readonly CoW page with a normal page.
    pub fn replace_cow_page(&mut self, addr: VirtAddr) {
        let page = Page::containing_address(addr);

        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();

        let new_frame = frame_allocator.allocate_frame().unwrap();
        let new_addr = apply_offset(new_frame.start_address().as_u64());

        let TranslateResult::Mapped { mut flags, .. } =
            self.page_table.inner.translate(page.start_address())
        else {
            return;
        };

        let (old_frame, flush) = self.page_table.inner.unmap(page).unwrap();
        flush.flush();

        flags.remove(COW_FLAG);
        flags |= PageTableFlags::WRITABLE;

        unsafe {
            copy_nonoverlapping(
                apply_offset(old_frame.start_address().as_u64()) as *const u8,
                new_addr as *mut u8,
                4096,
            );

            self.page_table
                .inner
                .map_to(page, new_frame, flags, &mut *frame_allocator)
                .unwrap()
                .flush()
        };
    }
}
