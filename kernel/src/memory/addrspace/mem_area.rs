use alloc::{sync::Arc, vec::Vec};
use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameDeallocator, Page, PageTableFlags, PhysFrame, Size4KiB, page::PageRange,
    },
};

use crate::{
    filesystem::object::FileLikeObject,
    memory::{
        addrspace::{USER_MEM_END, cow::decrease_ref},
        paging::FRAME_ALLOCATOR,
    },
};

#[derive(Debug)]
pub struct SharedFrames {
    frames: Arc<[PhysFrame]>,
}

impl SharedFrames {
    pub fn new(frames: Vec<PhysFrame>) -> Self {
        for frame in &frames {
            crate::memory::addrspace::cow::increase_ref(*frame);
        }
        Self {
            frames: Arc::from(frames),
        }
    }

    pub fn get(&self, index: usize) -> PhysFrame {
        self.frames[index]
    }
}

impl Drop for SharedFrames {
    fn drop(&mut self) {
        let mut allocator = FRAME_ALLOCATOR.get().unwrap().lock();
        for frame in self.frames.iter().copied() {
            if decrease_ref(frame) {
                unsafe {
                    allocator.deallocate_frame(frame);
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MemoryArea {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub flags: PageTableFlags,
    pub data: Data,
    pub lazy: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MmapPermissions {
    pub shared_write_allowed: Option<bool>,
}

// The data a memory area contains. Aka backing
#[derive(Clone, Debug)]
pub enum Data {
    // Normal data that a process/thread can write to. Aka anonymus.
    Normal(MmapPermissions),
    File {
        offset: u64,
        // Bytes from `offset` that belong to this mapping; the rest stays zeroed.
        file_bytes: u64,
        file: Arc<FileLikeObject>,
        shared: bool,
    },
    Shared {
        frames: Arc<SharedFrames>,
        flags: PageTableFlags,
    },
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
        self.start.as_u64() < USER_MEM_END && self.end.as_u64() <= USER_MEM_END
    }
}
