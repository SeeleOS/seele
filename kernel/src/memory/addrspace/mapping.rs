use alloc::vec::Vec;
use x86_64::{
    VirtAddr,
    structures::paging::{FrameDeallocator, Page, Size4KiB},
};

use crate::memory::{
    addrspace::{
        cow::decrease_ref,
        mem_area::{Data, MemoryArea},
    },
    paging::FRAME_ALLOCATOR,
};

use super::{AddrSpace, AllocResult, LAZY_MAP};

impl AddrSpace {
    pub fn register_area(&mut self, mut area: MemoryArea) -> Option<AllocResult> {
        log::trace!("addrspace: register area {:?}", area);

        if !LAZY_MAP {
            area.lazy = false;
        }

        // Keep metadata non-overlapping so page-fault lookup sees a single
        // definitive backing/permission source for each virtual page.
        self.unmap_areas(area.start, area.end);

        let insert_index = self
            .memory_areas
            .binary_search_by_key(&area.start, |existing| existing.start)
            .unwrap_or_else(|index| index);
        self.memory_areas.insert(insert_index, area.clone());
        self.last_area_index = None;

        if area.lazy {
            None
        } else {
            Some(self.apply_area(area))
        }
    }

    pub fn map(&mut self, area: MemoryArea) -> AllocResult {
        self.register_area(area)
            .expect("called map with a lazy mem area")
    }

    pub fn unmap(&mut self, start: VirtAddr, len: u64) {
        let _ = self.try_unmap(start, len);
    }

    pub fn try_unmap(
        &mut self,
        start: VirtAddr,
        len: u64,
    ) -> Result<(), crate::filesystem::errors::FSError> {
        if len == 0 {
            return Ok(());
        }

        self.flush_file_mappings(start, len)?;

        let end = start + len;
        let last_mapped_addr = end - 1u64;
        let mut changed = false;
        let mut frames_to_deallocate = Vec::new();

        for page in Page::<Size4KiB>::range_inclusive(
            Page::containing_address(start),
            Page::containing_address(last_mapped_addr),
        ) {
            if let Ok((frame, flush)) = self.page_table.unmap(page) {
                flush.flush();
                changed = true;
                if decrease_ref(frame) {
                    frames_to_deallocate.push(frame);
                }
            }
        }
        self.flush_page_table_updates(changed);
        if !frames_to_deallocate.is_empty() {
            let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
            for frame in frames_to_deallocate {
                unsafe {
                    frame_allocator.deallocate_frame(frame);
                }
            }
        }

        self.unmap_areas(start, end);
        Ok(())
    }

    // Unmaps the memory_areas inside AddrSpace, not the actual memory.
    fn unmap_areas(&mut self, start: VirtAddr, end: VirtAddr) {
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
                if let Data::File {
                    offset,
                    file_bytes,
                    zero_fill_after_file,
                    file,
                    shared,
                } = &area.data
                {
                    let span = left.end.as_u64() - left.start.as_u64();
                    left.data = Data::File {
                        offset: *offset,
                        file_bytes: (*file_bytes).min(span),
                        zero_fill_after_file: *zero_fill_after_file,
                        file: file.clone(),
                        shared: *shared,
                    };
                }
                new_areas.push(left);
            }

            if overlap_end < area_end {
                let mut right = area.clone();
                right.start = overlap_end;

                if let Data::File {
                    offset,
                    file_bytes,
                    zero_fill_after_file,
                    file,
                    shared,
                } = &area.data
                {
                    let span = right.end.as_u64() - right.start.as_u64();
                    right.data = Data::File {
                        offset: *offset + (overlap_end.as_u64() - area_start.as_u64()),
                        file_bytes: file_bytes
                            .saturating_sub(overlap_end.as_u64() - area_start.as_u64())
                            .min(span),
                        zero_fill_after_file: *zero_fill_after_file,
                        file: file.clone(),
                        shared: *shared,
                    };
                }

                new_areas.push(right);
            }
        }

        self.memory_areas = new_areas;
        self.last_area_index = None;
    }
}
