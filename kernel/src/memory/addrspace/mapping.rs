use alloc::vec::Vec;
use x86_64::{
    VirtAddr,
    structures::paging::{Mapper, Page, Size4KiB},
};

use crate::{
    memory::{
        addrspace::{
            mem_area::{Data, MemoryArea},
        },
    },
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
        if len == 0 {
            return;
        }

        let end = start + len;
        let last_mapped_addr = end - 1u64;

        for page in Page::<Size4KiB>::range_inclusive(
            Page::containing_address(start),
            Page::containing_address(last_mapped_addr),
        ) {
            if let Ok((_, flush)) = self.page_table.inner.unmap(page) {
                flush.flush();
            }
        }

        self.unmap_areas(start, end);
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
                    file,
                } = &area.data
                {
                    let span = left.end.as_u64() - left.start.as_u64();
                    left.data = Data::File {
                        offset: *offset,
                        file_bytes: (*file_bytes).min(span),
                        file: file.clone(),
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
                    file,
                } = &area.data
                {
                    let span = right.end.as_u64() - right.start.as_u64();
                    right.data = Data::File {
                        offset: *offset + (overlap_end.as_u64() - area_start.as_u64()),
                        file_bytes: file_bytes
                            .saturating_sub(overlap_end.as_u64() - area_start.as_u64())
                            .min(span),
                        file: file.clone(),
                    };
                }

                new_areas.push(right);
            }
        }

        self.memory_areas = new_areas;
        self.last_area_index = None;
    }
}
