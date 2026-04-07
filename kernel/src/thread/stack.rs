use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags},
};

use crate::{
    memory::{
        paging::{FRAME_ALLOCATOR, MAPPER},
        utils::apply_offset,
    },
    misc::stack_builder::StackBuilder,
};

static KERNEL_MEM: AtomicU64 = AtomicU64::new(0xFFFF_8000_3000_0000);

pub fn allocate_kernel_stack(pages: u64) -> StackBuilder {
    let guard_page = Page::containing_address(VirtAddr::new(
        KERNEL_MEM.fetch_add((pages + 1) * 4096, Ordering::Relaxed),
    ));
    let start = guard_page + 1;
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    let mut page_write_bases = alloc::vec::Vec::with_capacity(pages as usize);
    let mut frame_allocator = FRAME_ALLOCATOR.try_get().unwrap().lock();

    for i in 0..pages {
        let page = start + i;
        let frame = frame_allocator.allocate_frame().expect("Memory full.");

        unsafe {
            MAPPER
                .get()
                .unwrap()
                .lock()
                .map_to(page, frame, flags, &mut *frame_allocator)
                .unwrap()
                .flush();
        };

        let write_addr = apply_offset(frame.start_address().as_u64());
        unsafe {
            let bytes = 4096;
            let start_ptr = write_addr as *mut u8;
            core::ptr::write_bytes(start_ptr, 0, bytes as usize);
        }

        page_write_bases.push(write_addr);
    }

    let end_addr = (start + pages).start_address();

    StackBuilder::new(end_addr.as_u64(), page_write_bases)
}
