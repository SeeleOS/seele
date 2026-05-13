use crate::memory::utils::Mut;
use alloc::sync::Arc;
use bootloader_api::info::{MemoryRegion, MemoryRegionKind};
use buddy_system_allocator::FrameAllocator as BuddyFrameAllocator;
use conquer_once::spin::OnceCell;
use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::Cr3,
    structures::paging::{
        FrameAllocator, FrameDeallocator, OffsetPageTable, PageSize, PageTable, PhysFrame, Size4KiB,
    },
};

pub static MAPPER: OnceCell<Arc<Mut<OffsetPageTable<'static>>>> = OnceCell::uninit();
pub static FRAME_ALLOCATOR: OnceCell<Arc<Mut<BootinfoFrameAllocator>>> = OnceCell::uninit();
pub static HEAP_BACKING_ALLOCATOR: OnceCell<Mut<BootstrapFrameAllocator>> = OnceCell::uninit();
const LOWER_MEMORY_END: u64 = 0x10_0000;
const AP_TRAMPOLINE_END: u64 = 0x9000;
const FRAME_ALLOCATOR_ORDER: usize = 33;

pub fn init_mapper(physcal_memory_offset: u64) -> OffsetPageTable<'static> {
    unsafe {
        OffsetPageTable::new(
            get_l4_table(VirtAddr::new(physcal_memory_offset)),
            VirtAddr::new(physcal_memory_offset),
        )
    }
}

pub fn get_l4_table(phys_mem_offset: VirtAddr) -> &'static mut PageTable {
    let addr = Cr3::read().0.start_address();
    let virt = phys_mem_offset + addr.as_u64();
    let page_table_ptr = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

#[derive(Clone)]
pub struct BootstrapFrameAllocator {
    memory_map: &'static [MemoryRegion],
    next_region_index: usize,
    next_frame_addr: u64,
}

impl BootstrapFrameAllocator {
    /// # Safety
    ///
    /// `memory_map` must remain valid for the allocator's lifetime and must
    /// describe physical memory that is not concurrently mutated elsewhere.
    pub unsafe fn new(memory_map: &'static [MemoryRegion]) -> Self {
        Self {
            memory_map,
            next_region_index: 0,
            next_frame_addr: LOWER_MEMORY_END,
        }
    }

    fn next_usable_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        while let Some(region) = self.memory_map.get(self.next_region_index) {
            if region.kind != MemoryRegionKind::Usable {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
                continue;
            }

            let start = align_up_4k(region.start.max(LOWER_MEMORY_END));
            let end = align_down_4k(region.end);

            if start >= end {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
                continue;
            }

            if self.next_frame_addr == 0 || self.next_frame_addr < start {
                self.next_frame_addr = start;
            }

            if self.next_frame_addr >= end {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
                continue;
            }

            let addr = self.next_frame_addr;
            self.next_frame_addr = self.next_frame_addr.saturating_add(4096);

            return Some(PhysFrame::containing_address(PhysAddr::new(addr)));
        }

        None
    }

    pub fn reserve_pages(&mut self, mut pages: usize) -> Option<()> {
        while let Some(region) = self.memory_map.get(self.next_region_index) {
            if region.kind != MemoryRegionKind::Usable {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
                continue;
            }

            let start = align_up_4k(region.start.max(LOWER_MEMORY_END));
            let end = align_down_4k(region.end);

            if start >= end {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
                continue;
            }

            if self.next_frame_addr == 0 || self.next_frame_addr < start {
                self.next_frame_addr = start;
            }

            let available_pages = ((end - self.next_frame_addr) / Size4KiB::SIZE) as usize;
            if available_pages == 0 {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
                continue;
            }

            let consume_pages = available_pages.min(pages);
            self.next_frame_addr = self
                .next_frame_addr
                .saturating_add((consume_pages as u64) * Size4KiB::SIZE);
            pages -= consume_pages;

            if pages == 0 {
                return Some(());
            }

            if self.next_frame_addr >= end {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
            }
        }

        None
    }

    pub fn clone_after_reserving(&self, bytes: usize) -> Option<Self> {
        let mut cloned = self.clone();
        let pages = bytes.div_ceil(Size4KiB::SIZE as usize);
        cloned.reserve_pages(pages)?;
        Some(cloned)
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootstrapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.next_usable_frame()
    }
}

pub struct BootinfoFrameAllocator {
    inner: BuddyFrameAllocator<FRAME_ALLOCATOR_ORDER>,
}

impl BootinfoFrameAllocator {
    pub fn new(memory_map: &'static [MemoryRegion], cursor: &BootstrapFrameAllocator) -> Self {
        let mut allocator = Self {
            inner: BuddyFrameAllocator::new(),
        };

        for (index, region) in memory_map.iter().enumerate() {
            if region.kind != MemoryRegionKind::Usable {
                continue;
            }

            let start = align_up_4k(region.start.max(AP_TRAMPOLINE_END));
            let end = align_down_4k(region.end);
            if start >= end {
                continue;
            }

            let free_start = if index < cursor.next_region_index {
                continue;
            } else if index == cursor.next_region_index {
                start.max(align_up_4k(cursor.next_frame_addr))
            } else {
                start
            };

            if free_start < end {
                allocator.add_free_region(free_start, end);
            }
        }

        allocator
    }

    fn add_free_region(&mut self, start: u64, end: u64) {
        let start_frame = (start / Size4KiB::SIZE) as usize;
        let end_frame = (end / Size4KiB::SIZE) as usize;
        if start_frame < end_frame {
            self.inner.add_frame(start_frame, end_frame);
        }
    }

    pub(crate) fn normalized_pages(pages: usize) -> Option<usize> {
        if pages == 0 {
            return None;
        }
        Some(pages.next_power_of_two())
    }

    pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame<Size4KiB>> {
        let pages = Self::normalized_pages(pages)?;
        let frame_number = self.inner.alloc(pages)?;
        Some(PhysFrame::containing_address(PhysAddr::new(
            (frame_number as u64) * Size4KiB::SIZE,
        )))
    }

    /// # Safety
    ///
    /// The caller must ensure the range `[start, start + pages)` was
    /// previously allocated from this allocator and is not still in use.
    pub unsafe fn deallocate_contiguous(&mut self, start: PhysFrame<Size4KiB>, pages: usize) {
        let Some(pages) = Self::normalized_pages(pages) else {
            return;
        };
        let start_frame = (start.start_address().as_u64() / Size4KiB::SIZE) as usize;
        self.inner.dealloc(start_frame, pages);
    }
}

pub(crate) const fn align_up_4k(addr: u64) -> u64 {
    (addr + 4095) & !4095
}

pub(crate) const fn align_down_4k(addr: u64) -> u64 {
    addr & !4095
}

unsafe impl FrameAllocator<Size4KiB> for BootinfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.allocate_contiguous(1)
    }
}

impl FrameDeallocator<Size4KiB> for BootinfoFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        let start_frame = (frame.start_address().as_u64() / Size4KiB::SIZE) as usize;
        self.inner.dealloc(start_frame, 1);
    }
}
