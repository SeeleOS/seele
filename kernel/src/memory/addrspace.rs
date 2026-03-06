use alloc::vec::Vec;
use futures_util::stream::All;
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Translate},
};

use crate::{
    memory::{
        page_table_wrapper::PageTableWrapped,
        paging::{FRAME_ALLOCATOR, MAPPER},
        utils::{MemoryRegion, apply_offset},
    },
    misc::stack_builder::StackBuilder,
    s_print, s_println,
};

const USER_MEM_START: u64 = 0x30_0000_0000;
const KERNEL_MEM_START: u64 = 0xFFFF_8000_1000_0000;

pub type AllocResult = (VirtAddr, StackBuilder);

#[derive(Debug)]
pub struct AddrSpace {
    used_memories: Vec<MemoryRegion>,
    page_table: PageTableWrapped,

    user_mem: VirtAddr,
    kernel_mem: VirtAddr,
}

impl Default for AddrSpace {
    fn default() -> Self {
        Self {
            used_memories: Vec::default(),
            page_table: PageTableWrapped::default(),
            user_mem: VirtAddr::new(USER_MEM_START),
            kernel_mem: VirtAddr::new(KERNEL_MEM_START),
        }
    }
}

impl AddrSpace {
    pub fn load(&mut self) {
        self.page_table.load();
    }

    pub fn translate_addr(&self, addr: VirtAddr) -> Option<PhysAddr> {
        self.page_table.inner.translate_addr(addr)
    }

    pub fn allocate_user(&mut self, pages: u64) -> AllocResult {
        let mem = self.user_mem;
        self.user_mem += (pages + 1) * 4096;

        self.map(
            mem,
            pages,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
        )
    }

    pub fn allocate_kernel(&mut self, pages: u64) -> AllocResult {
        let mem = self.kernel_mem;
        self.kernel_mem += (pages + 1) * 4096;

        self.map(
            mem,
            pages,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
        )
    }

    pub fn map(&mut self, start: VirtAddr, pages: u64, flags: PageTableFlags) -> AllocResult {
        let region = MemoryRegion::new(start, pages, flags);
        s_println!("Mapping VAddr: {:?} to {:?} pages", start, pages);

        self.used_memories.push(region);

        self.apply_region(region, true)
    }

    pub fn map_no_guard_page(
        &mut self,
        start: VirtAddr,
        pages: u64,
        flags: PageTableFlags,
    ) -> AllocResult {
        let region = MemoryRegion::new(start, pages, flags);

        self.used_memories.push(region);

        self.apply_region(region, false)
    }

    fn apply_region(&mut self, region: MemoryRegion, use_guard_page: bool) -> AllocResult {
        let guard_page = region.start_page();
        let start = {
            if use_guard_page {
                guard_page + 1
            } else {
                guard_page
            }
        };
        let pages = region.pages();

        let mut last_frame = None;
        let mut frame_allocator = FRAME_ALLOCATOR.try_get().unwrap().lock();

        for i in 0..pages {
            let page = start + i;
            let frame = frame_allocator.allocate_frame().expect("Memory full.");

            unsafe {
                self.page_table
                    .inner
                    .map_to(page, frame, region.flags, &mut *frame_allocator)
                    .unwrap()
                    .flush();
            };

            let write_addr = apply_offset(frame.start_address().as_u64() + 4096);
            unsafe {
                let bytes = 4096;
                let start_ptr = (write_addr as usize - bytes as usize) as *mut u8;
                core::ptr::write_bytes(start_ptr, 0, bytes as usize);
            }

            last_frame = Some(frame);
        }

        let start_addr = start.start_address();
        let end_addr = (start + pages).start_address();
        let write_addr = apply_offset(last_frame.unwrap().start_address().as_u64() + 4096);

        (
            start_addr,
            StackBuilder::new(end_addr.as_u64(), write_addr as *mut u64),
        )
    }
}
