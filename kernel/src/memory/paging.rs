use alloc::sync::Arc;
use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use conquer_once::spin::OnceCell;
use spin::Mutex;
use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::Cr3,
    structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB},
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
#[derive(Clone, Copy)]
pub struct BootinfoFrameAllocator {
    memory_map: &'static MemoryRegions,
    index: usize,
}

impl BootinfoFrameAllocator {
    pub unsafe fn new(memory_map: &'static MemoryRegions) -> Self {
        Self {
            memory_map,
            index: 0,
        }
    }

    fn get_usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        let regions = self.memory_map.iter();
        let usable_regions = regions.filter(|r| r.kind == MemoryRegionKind::Usable);

        // Converts a list of usable regions into a list of addresses of usable regions
        let usable_regions_addr = usable_regions.map(|r| r.start..r.end);
        // aglien them. note to future me: i also dont know wtf is
        // happening here, just ask AI or something lolz
        let frame_addresses = usable_regions_addr.flat_map(|r| r.step_by(4096));
        // convert them into physical frames
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }

    fn next_usable_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = self.get_usable_frames().nth(self.index);
        self.index += 1;
        frame
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootinfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.next_usable_frame()
    }
}
