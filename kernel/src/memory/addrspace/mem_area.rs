use x86_64::{
    VirtAddr,
    structures::paging::{Page, PageTable, PageTableFlags, Size4KiB, page::PageRangeInclusive},
};

#[derive(Clone, Copy, Debug)]
pub struct MemoryArea {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub flags: PageTableFlags,
    pub data: Data,
    pub lazy: bool,
}

// The data a memory area contains. Aka backing
#[derive(Clone, Copy, Debug)]
pub enum Data {
    // Normal data that a process/thread can write to. Aka anonymus.
    Normal,
}

impl MemoryArea {
    pub fn new(start: VirtAddr, pages: u64, flags: PageTableFlags, data: Data, lazy: bool) -> Self {
        Self {
            start,
            end: start + (pages * 4096),
            flags,
            data,
            lazy,
        }
    }

    pub fn new_with_guard(
        start: VirtAddr,
        pages: u64,
        flags: PageTableFlags,
        data: Data,
        lazy: bool,
    ) -> Self {
        Self::new(start + 4096, pages, flags, data, lazy)
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

    pub fn contains(&self, addr: VirtAddr) -> bool {
        addr >= self.start && addr < self.end
    }
}
