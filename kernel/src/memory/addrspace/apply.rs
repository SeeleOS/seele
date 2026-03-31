use crate::memory::addrspace::{AddrSpace, AllocResult};
use alloc::{slice, sync::Arc, vec::Vec};
use seele_sys::permission::Permissions;
use spleen_font::Size;
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB},
};

use crate::{
    filesystem::object::FileLikeObject,
    memory::{
        addrspace::{
            cow::increase_ref,
            mem_area::{Data, MemoryArea},
        },
        paging::FRAME_ALLOCATOR,
        utils::apply_offset,
    },
    misc::{others::permissions_to_flags, stack_builder::StackBuilder},
    object::misc::ObjectRef,
};

impl AddrSpace {
    pub fn apply_page(&mut self, page: Page<Size4KiB>, area: MemoryArea) -> PhysFrame {
        match area.data {
            Data::Normal => self.alloc_map_zeroed_page(page, area).0,
            Data::File { offset, ref file } => unsafe {
                let (frame, write_addr) = self.alloc_map_zeroed_page(page, area.clone());

                let info = file.info().unwrap();
                let file_size = info.size as u64;
                let offset_in_area = offset + (page.start_address().as_u64() - area.start.as_u64());
                let read_len = core::cmp::min(4096, file_size.saturating_sub(offset_in_area));

                file.read_exact_at(
                    slice::from_raw_parts_mut(write_addr as *mut u8, read_len as usize),
                    offset_in_area,
                )
                .expect("Failed to lazyload page with file data");

                frame
            },
            Data::Shared { start } => unsafe {
                let page_index = (page.start_address().as_u64() - area.start.as_u64()) / 4096;
                let frame = start + page_index;

                self.page_table
                    .inner
                    .map_to(
                        page,
                        frame,
                        area.flags,
                        &mut *FRAME_ALLOCATOR.get().unwrap().lock(),
                    )
                    .unwrap()
                    .flush();

                frame
            },
        }
    }

    pub fn apply_area(&mut self, area: MemoryArea) -> AllocResult {
        log::trace!(
            "addrspace: apply_region start {:#x} pages {}",
            area.start.as_u64(),
            area.pages()
        );
        let start = area.start_page();
        let pages = area.pages();

        let mut last_frame = None;

        for i in 0..pages {
            let page = start + i;
            last_frame = Some(self.apply_page(page, area.clone()));
        }

        let start_addr = start.start_address();
        let end_addr = (start + pages).start_address();
        let write_addr = apply_offset(last_frame.unwrap().start_address().as_u64() + 4096);

        (
            start_addr,
            StackBuilder::new(end_addr.as_u64(), write_addr as *mut u8),
        )
    }

    fn alloc_map_zeroed_page(
        &mut self,
        page: Page<Size4KiB>,
        area: MemoryArea,
    ) -> (PhysFrame, u64) {
        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
        let frame = frame_allocator.allocate_frame().expect("memory full;");

        unsafe {
            self.page_table
                .inner
                .map_to(page, frame, area.flags, &mut *frame_allocator)
                .unwrap()
                .flush();
        };

        let write_addr = apply_offset(frame.start_address().as_u64());
        increase_ref(frame);

        unsafe {
            let start_ptr = (write_addr as usize) as *mut u8;
            core::ptr::write_bytes(start_ptr, 0, 4096);
        }

        (frame, write_addr)
    }
}
