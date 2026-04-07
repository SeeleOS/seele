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
                    right.data = Data::File {
                        offset: *offset + (overlap_end.as_u64() - area_start.as_u64()),
                        file_bytes: file_bytes
                            .saturating_sub(overlap_end.as_u64() - area_start.as_u64()),
                        file: file.clone(),
                    };
                }

                new_areas.push(right);
            }
        }

        self.memory_areas = new_areas;
    }
}
