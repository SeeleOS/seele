use x86_64::{
    VirtAddr,
    structures::paging::{Page, PageTable, PageTableFlags, Size4KiB, page::PageRangeInclusive},
};

#[derive(Clone, Copy, Debug)]
pub struct MemoryArea {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub flags: PageTableFlags,
}

impl MemoryArea {
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
