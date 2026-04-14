use alloc::{slice, vec::Vec};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PhysFrame, Size4KiB, Translate,
};

use crate::{
    memory::{
        addrspace::{
            cow::increase_ref,
            mem_area::{Data, MemoryArea},
            AddrSpace, AllocResult,
        },
        paging::FRAME_ALLOCATOR,
        utils::apply_offset,
    },
    misc::{
        stack_builder::StackBuilder,
        time::Time,
    },
};

const FILE_LAZY_CLUSTER_PAGES: u64 = 4;

impl AddrSpace {
    pub fn apply_page(&mut self, page: Page<Size4KiB>, area: MemoryArea) -> PhysFrame {
        self.apply_page_cluster(page, area, 1)
    }

    pub fn apply_page_cluster(
        &mut self,
        page: Page<Size4KiB>,
        area: MemoryArea,
        cluster_pages: u64,
    ) -> PhysFrame {
        match area.data {
            Data::Normal => self.alloc_map_zeroed_page(page, area, true).0,
            Data::File {
                offset,
                file_bytes,
                ref file,
            } => unsafe {
                let page_index = (page.start_address().as_u64() - area.start.as_u64()) / 4096;
                let max_pages =
                    core::cmp::min(cluster_pages, area.pages().saturating_sub(page_index));
                let mut first_frame = None;

                for i in 0..max_pages {
                    let current_page = page + i;
                    if self
                        .page_table
                        .inner
                        .translate_addr(current_page.start_address())
                        .is_some()
                    {
                        continue;
                    }

                    let page_offset = current_page.start_address().as_u64() - area.start.as_u64();
                    let read_len = core::cmp::min(4096, file_bytes.saturating_sub(page_offset));
                    let (frame, write_addr) = self.alloc_map_zeroed_page(
                        current_page,
                        area.clone(),
                        read_len < 4096,
                    );

                    if first_frame.is_none() {
                        first_frame = Some(frame);
                    }

                    if read_len != 0 {
                        let read_start = Time::since_boot();
                        file.read_exact_at(
                            slice::from_raw_parts_mut(write_addr as *mut u8, read_len as usize),
                            offset + page_offset,
                        )
                        .expect("Failed to lazyload page with file data");
                        let _ = Time::since_boot().sub(read_start);
                    }
                }

                first_frame.expect("file-backed cluster fault mapped no pages")
            },
            Data::Shared {
                ref frames,
                flags: shared_flags,
            } => unsafe {
                let page_index = (page.start_address().as_u64() - area.start.as_u64()) / 4096;
                let frame = frames[page_index as usize];

                self.page_table
                    .inner
                    .map_to(
                        page,
                        frame,
                        area.flags | shared_flags,
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

        let mut page_write_bases = Vec::with_capacity(pages as usize);

        match area.data {
            Data::File { .. } => {
                for i in 0..pages {
                    let page = start + i;
                    let frame = self.apply_page_cluster(page, area.clone(), 1);
                    page_write_bases.push(apply_offset(frame.start_address().as_u64()));
                }
            }
            Data::Normal => {
                for i in 0..pages {
                    let page = start + i;
                    let frame = self.alloc_map_zeroed_page(page, area.clone(), true).0;
                    page_write_bases.push(apply_offset(frame.start_address().as_u64()));
                }
            }
            Data::Shared { .. } => {
                for i in 0..pages {
                    let page = start + i;
                    let frame = self.apply_page(page, area.clone());
                    page_write_bases.push(apply_offset(frame.start_address().as_u64()));
                }
            }
        }

        let start_addr = start.start_address();
        let end_addr = (start + pages).start_address();

        (start_addr, StackBuilder::new(end_addr.as_u64(), page_write_bases))
    }

    pub fn file_lazy_cluster_pages() -> u64 {
        FILE_LAZY_CLUSTER_PAGES
    }

    fn alloc_map_zeroed_page(
        &mut self,
        page: Page<Size4KiB>,
        area: MemoryArea,
        zero_page: bool,
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

        if zero_page {
            unsafe {
                let start_ptr = (write_addr as usize) as *mut u8;
                core::ptr::write_bytes(start_ptr, 0, 4096);
            }
        }

        (frame, write_addr)
    }
}
