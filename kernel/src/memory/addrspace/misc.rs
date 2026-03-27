use x86_64::VirtAddr;

use crate::memory::addrspace::{
    AddrSpace,
    mem_area::{Data, MemoryArea},
};

impl AddrSpace {
    pub fn fetch_add_user_mem(&mut self, pages: u64) -> VirtAddr {
        let mem = self.user_mem;
        self.user_mem += (pages + 1) * 4096;
        mem
    }
}

/// Remove the overlapped part of [start, end) from [`MemoryArea`] and return the
/// remaining left/right pieces. Callers can either drop the middle part
/// (munmap) or recreate it with different metadata/permissions (mprotect).
pub fn split_memory_area(
    area: &MemoryArea,
    start: VirtAddr,
    end: VirtAddr,
) -> (Option<MemoryArea>, Option<MemoryArea>) {
    let overlap_start = core::cmp::max(area.start, start);
    let overlap_end = core::cmp::min(area.end, end);

    if overlap_start >= overlap_end {
        return (Some(area.clone()), None);
    }

    let left = if area.start < overlap_start {
        let mut left = area.clone();
        left.end = overlap_start;
        Some(left)
    } else {
        None
    };

    let right = if overlap_end < area.end {
        let mut right = area.clone();
        right.start = overlap_end;

        if let Data::File { offset, file } = &area.data {
            right.data = Data::File {
                offset: *offset + (overlap_end.as_u64() - area.start.as_u64()),
                file: file.clone(),
            };
        }

        Some(right)
    } else {
        None
    };

    (left, right)
}
