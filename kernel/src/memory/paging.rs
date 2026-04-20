use alloc::{sync::Arc, vec::Vec};
use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use conquer_once::spin::OnceCell;
use spin::Mutex;
use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::Cr3,
    structures::paging::{
        FrameAllocator, FrameDeallocator, OffsetPageTable, PageSize, PageTable, PhysFrame, Size4KiB,
    },
};

pub static MAPPER: OnceCell<Arc<Mutex<OffsetPageTable<'static>>>> = OnceCell::uninit();
pub static FRAME_ALLOCATOR: OnceCell<Arc<Mutex<BootinfoFrameAllocator>>> = OnceCell::uninit();

// initalize the mapper thats based on offset page table
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

// allocates avalible frames based on bootinfos memory map
#[derive(Clone)]
pub struct BootinfoFrameAllocator {
    memory_map: &'static MemoryRegions,
    free_frames: Vec<PhysFrame<Size4KiB>>,
    next_region_index: usize,
    next_frame_addr: u64,
}

impl BootinfoFrameAllocator {
    /// # Safety
    ///
    /// `memory_map` must remain valid for the allocator's lifetime and must
    /// describe physical memory that is not concurrently mutated elsewhere.
    pub unsafe fn new(memory_map: &'static MemoryRegions) -> Self {
        Self {
            memory_map,
            free_frames: Vec::new(),
            next_region_index: 0,
            next_frame_addr: 0,
        }
    }

    fn next_usable_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        if let Some(frame) = self.free_frames.pop() {
            return Some(frame);
        }

        while let Some(region) = self.memory_map.get(self.next_region_index) {
            if region.kind != MemoryRegionKind::Usable {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
                continue;
            }

            let start = align_up_4k(region.start);
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

    fn take_contiguous_free_frames(&mut self, pages: usize) -> Option<PhysFrame<Size4KiB>> {
        if self.free_frames.len() < pages {
            return None;
        }

        self.free_frames
            .sort_unstable_by_key(|frame| frame.start_address().as_u64());

        let mut run_start = 0usize;
        let mut run_len = 1usize;

        for i in 1..self.free_frames.len() {
            let prev = self.free_frames[i - 1].start_address().as_u64();
            let current = self.free_frames[i].start_address().as_u64();

            if current == prev + Size4KiB::SIZE {
                run_len += 1;
            } else {
                run_start = i;
                run_len = 1;
            }

            if run_len == pages {
                let first = run_start + 1 - pages;
                let start = self.free_frames[first];
                self.free_frames.drain(first..=run_start);
                return Some(start);
            }
        }

        None
    }

    fn next_contiguous_usable_frames(&mut self, pages: usize) -> Option<PhysFrame<Size4KiB>> {
        let span = (pages as u64).checked_mul(Size4KiB::SIZE)?;

        while let Some(region) = self.memory_map.get(self.next_region_index) {
            if region.kind != MemoryRegionKind::Usable {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
                continue;
            }

            let start = align_up_4k(region.start);
            let end = align_down_4k(region.end);

            if start >= end {
                self.next_region_index += 1;
                self.next_frame_addr = 0;
                continue;
            }

            if self.next_frame_addr == 0 || self.next_frame_addr < start {
                self.next_frame_addr = start;
            }

            let candidate = align_up_4k(self.next_frame_addr);
            let candidate_end = candidate.checked_add(span)?;

            if candidate_end <= end {
                self.next_frame_addr = candidate_end;
                return Some(PhysFrame::containing_address(PhysAddr::new(candidate)));
            }

            self.next_region_index += 1;
            self.next_frame_addr = 0;
        }

        None
    }

    pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame<Size4KiB>> {
        if pages == 0 {
            return None;
        }

        if pages == 1 {
            return self.allocate_frame();
        }

        self.take_contiguous_free_frames(pages)
            .or_else(|| self.next_contiguous_usable_frames(pages))
    }

    /// # Safety
    ///
    /// The caller must ensure the range `[start, start + pages)` was
    /// previously allocated from this allocator and is not still in use.
    pub unsafe fn deallocate_contiguous(&mut self, start: PhysFrame<Size4KiB>, pages: usize) {
        for page in 0..pages {
            let frame = PhysFrame::containing_address(PhysAddr::new(
                start.start_address().as_u64() + (page as u64 * Size4KiB::SIZE),
            ));
            unsafe {
                self.deallocate_frame(frame);
            }
        }
    }
}

const fn align_up_4k(addr: u64) -> u64 {
    (addr + 4095) & !4095
}

const fn align_down_4k(addr: u64) -> u64 {
    addr & !4095
}

unsafe impl FrameAllocator<Size4KiB> for BootinfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.next_usable_frame()
    }
}

impl FrameDeallocator<Size4KiB> for BootinfoFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        self.free_frames.push(frame);
    }
}
