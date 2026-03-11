use futures_util::stream::All;
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, PageTableFlags},
};

use crate::{
    memory::{
        addrspace::{AddrSpace, AllocResult},
        paging::FRAME_ALLOCATOR,
        utils::{MemoryRegion, apply_offset},
    },
    misc::stack_builder::StackBuilder,
};

impl AddrSpace {
    pub fn map(&mut self, start: VirtAddr, pages: u64, flags: PageTableFlags) -> AllocResult {
        log::trace!("addrspace: map guard start {:#x} pages {}", start.as_u64(), pages);
        let actual_start = start + 4096;
        let region = MemoryRegion::new(actual_start, pages, flags);

        self.used_memories.push(region);

        self.apply_region(region)
    }

    pub fn map_no_guard_page(
        &mut self,
        start: VirtAddr,
        pages: u64,
        flags: PageTableFlags,
    ) -> AllocResult {
        log::trace!(
            "addrspace: map_no_guard_page start {:#x} pages {}",
            start.as_u64(),
            pages
        );
        let region = MemoryRegion::new(start, pages, flags);

        self.used_memories.push(region);

        self.apply_region(region)
    }

    fn apply_region(&mut self, region: MemoryRegion) -> AllocResult {
        log::trace!(
            "addrspace: apply_region start {:#x} pages {}",
            region.start.as_u64(),
            region.pages()
        );
        let start = region.start_page();
        let pages = region.pages();

        let mut last_frame = None;
        let mut frame_allocator = FRAME_ALLOCATOR.try_get().unwrap().lock();

        for i in 0..pages {
            let page = start + i;
            let frame = frame_allocator.allocate_frame().expect("Memory full.");

            unsafe {
                self.page_table
                    .inner
                    .map_to(page, frame, region.flags, &mut *frame_allocator)
                    .unwrap()
                    .flush();
            };

            let write_addr = apply_offset(frame.start_address().as_u64() + 4096);
            unsafe {
                let bytes = 4096;
                let start_ptr = (write_addr as usize - bytes as usize) as *mut u8;
                core::ptr::write_bytes(start_ptr, 0, bytes as usize);
            }

            last_frame = Some(frame);
        }

        let start_addr = start.start_address();
        let end_addr = (start + pages).start_address();
        let write_addr = apply_offset(last_frame.unwrap().start_address().as_u64() + 4096);

        (
            start_addr,
            StackBuilder::new(end_addr.as_u64(), write_addr as *mut u64),
        )
    }
}
