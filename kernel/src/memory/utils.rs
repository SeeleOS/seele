use x86_64::{
    VirtAddr,
    structures::paging::{Page, PageTable, PageTableFlags, Size4KiB, page::PageRangeInclusive},
};

use crate::memory::{PHYSICAL_MEMORY_OFFSET, paging::MAPPER};

pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: spin::Mutex::new(inner),
        }
    }

    pub fn lock(&self) -> spin::MutexGuard<'_, A> {
        self.inner.lock()
    }
}

/// Copies the memory mapping of the kernel l4 table
/// into the table of something else (probably table for processes)
pub fn copy_kernel_mapping(table: &mut PageTable) {
    let l4_binding = MAPPER.get().unwrap().lock();
    let kernel_l4 = l4_binding.level_4_table();
    //s_println!("{:#?}", kernel_l4[0]);

    for i in 128..512 {
        table[i] = kernel_l4[i].clone();
    }
}

pub fn apply_offset(num: u64) -> u64 {
    num + PHYSICAL_MEMORY_OFFSET.get().unwrap()
}

pub fn page_range_from_size(start: u64, size: u64) -> PageRangeInclusive {
    page_range_from_addr(start, start + size - 1u64)
}

pub fn page_range_from_addr(start: u64, end: u64) -> PageRangeInclusive {
    let heap_start = VirtAddr::new(start);
    let heap_end = VirtAddr::new(end);
    let heap_start_page = Page::containing_address(heap_start);
    let heap_end_page = Page::containing_address(heap_end);
    Page::range_inclusive(heap_start_page, heap_end_page)
}

#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub flags: PageTableFlags,
}

impl MemoryRegion {
    pub fn new(start: VirtAddr, pages: u64, flags: PageTableFlags) -> Self {
        Self {
            start,
            end: start + (pages * 4096),
            flags,
        }
    }

    pub fn pages(&self) -> u64 {
        (self.end - self.start) / 4096
    }

    pub fn start_page(&self) -> Page<Size4KiB> {
        Page::containing_address(self.start)
    }

    pub fn end_page(&self) -> Page<Size4KiB> {
        Page::containing_address(self.end)
    }
}
