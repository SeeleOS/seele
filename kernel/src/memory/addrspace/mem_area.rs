use x86_64::{
    VirtAddr,
    structures::paging::{
        Page, PageTableFlags, Size4KiB,
        page::{PageRange, PageRangeInclusive},
    },
};

use crate::{filesystem::path::Path, memory::addrspace::KERNEL_MEM_START, object::misc::ObjectRef};

#[derive(Clone, Debug)]
pub struct MemoryArea {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub flags: PageTableFlags,
    pub data: Data,
    pub lazy: bool,
}

// The data a memory area contains. Aka backing
#[derive(Clone, Debug)]
pub enum Data {
    // Normal data that a process/thread can write to. Aka anonymus.
    Normal,
    File { offset: u64, file: ObjectRef },
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

    pub fn page_range(&self) -> PageRange<Size4KiB> {
        Page::range(self.start_page(), self.end_page())
    }

    pub fn contains(&self, addr: VirtAddr) -> bool {
        addr >= self.start && addr < self.end
    }

    pub fn is_user(&self) -> bool {
        self.start.as_u64() < KERNEL_MEM_START
    }
}
