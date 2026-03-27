use alloc::{slice, sync::Arc, vec::Vec};
use seele_sys::permission::Permissions;
use spleen_font::Size;
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

    pub fn unmap(&mut self, start: VirtAddr, end: VirtAddr) {
        for page in Page::<Size4KiB>::range_inclusive(
            Page::containing_address(start),
            Page::containing_address(end),
        ) {
            if let Ok((_, flush)) = self.page_table.inner.unmap(page) {
                flush.flush();
            }
        }

        let mut new_areas = Vec::new();

        for area in self.memory_areas.drain(..) {
            let area_start = area.start;
            let area_end = area.end;

            let overlap_start = core::cmp::max(area_start, start);
            let overlap_end = core::cmp::min(area_end, end);

            if overlap_start >= overlap_end {
                new_areas.push(area);
                continue;
            }

            if area_start < overlap_start {
                let mut left = area.clone();
                left.end = overlap_start;
                new_areas.push(left);
            }

            if overlap_end < area_end {
                let mut right = area.clone();
                right.start = overlap_end;

                if let Data::File { offset, file } = &area.data {
                    right.data = Data::File {
                        offset: *offset + (overlap_end.as_u64() - area_start.as_u64()),
                        file: file.clone(),
                    };
                }

                new_areas.push(right);
            }
        }

        self.memory_areas = new_areas;
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

        match area.data {
            Data::Normal => {}
            Data::File { offset, file } => unsafe {
                let info = file.info().unwrap();
                let file_size = info.size as u64;
                let offset_in_area = offset + (page.start_address().as_u64() - area.start.as_u64());
                let read_len = core::cmp::min(4096, file_size.saturating_sub(offset_in_area));

                file.read_exact_at(
                    slice::from_raw_parts_mut(write_addr as *mut u8, read_len as usize),
                    offset_in_area,
                )
                .expect("Failed to lazyload page with file data");
            },
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
