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
};

const USER_MEM_START: u64 = 0x30_0000_0000;
const KERNEL_MEM_START: u64 = 0xFFFF_8000_1000_0000;

pub type AllocResult = (VirtAddr, StackBuilder);

#[derive(Debug)]
pub struct AddrSpace {
    pub used_memories: Vec<MemoryRegion>,
    pub page_table: PageTableWrapped,

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
}
