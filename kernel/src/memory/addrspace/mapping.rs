use core::simd::cmp;

use alloc::{slice, sync::Arc};
use seele_sys::permission::Permissions;
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB},
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

use super::{AddrSpace, AllocResult};

impl AddrSpace {
    pub fn map(&mut self, area: MemoryArea) -> AllocResult {
        log::trace!("addrspace: mapping {:?}", area);
        self.memory_areas.push(area.clone());

        if !area.lazy {
            self.apply_area(area)
        } else {
            panic!("called map with a lazy mem area")
        }
    }

    pub fn map_lazy(&mut self, area: MemoryArea) -> VirtAddr {
        self.memory_areas.push(area.clone());

        area.start
    }

    pub fn map_file(
        &mut self,
        file: Arc<FileLikeObject>,
        offset: u64,
        pages: u64,
        permissions: Permissions,
    ) -> VirtAddr {
        let mem = self.fetch_add_user_mem(pages);
        self.map_lazy(MemoryArea::new(
            mem,
            pages,
            permissions_to_flags(permissions),
            Data::File { offset, file },
            true,
        ))
    }

    pub fn apply_page(&mut self, page: Page<Size4KiB>, area: MemoryArea) {
        match area.data {
            Data::Normal => {
                let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
                let frame = frame_allocator.allocate_frame().expect("memory full;");

                unsafe {
                    self.page_table
                        .inner
                        .map_to(page, frame, area.flags, &mut *frame_allocator)
                        .unwrap()
                        .flush();
                };

                let write_addr = apply_offset(frame.start_address().as_u64() + 4096);
                unsafe {
                    let bytes = 4096;
                    let start_ptr = (write_addr as usize - bytes as usize) as *mut u8;
                    core::ptr::write_bytes(start_ptr, 0, bytes as usize);
                }

                increase_ref(frame);
            }
            _ => todo!(),
        }
    }

    fn apply_area(&mut self, area: MemoryArea) -> AllocResult {
        log::trace!(
            "addrspace: apply_region start {:#x} pages {}",
            area.start.as_u64(),
            area.pages()
        );
        let start = area.start_page();
        let pages = area.pages();

        let mut last_frame = None;
        let mut frame_allocator = FRAME_ALLOCATOR.try_get().unwrap().lock();

        for i in 0..pages {
            let page = start + i;
            let frame = frame_allocator.allocate_frame().expect("Memory full.");

            unsafe {
                self.page_table
                    .inner
                    .map_to(page, frame, area.flags, &mut *frame_allocator)
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

            increase_ref(frame);
        }

        let start_addr = start.start_address();
        let end_addr = (start + pages).start_address();
        let write_addr = apply_offset(last_frame.unwrap().start_address().as_u64() + 4096);

        (
            start_addr,
            StackBuilder::new(end_addr.as_u64(), write_addr as *mut u8),
        )
    }
}
