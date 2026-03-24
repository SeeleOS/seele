use core::sync::atomic::{AtomicU64, Ordering};

use alloc::vec::Vec;
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{PageTableFlags, Translate},
};

use crate::{
    memory::{
        addrspace::mem_area::{Data, MemoryArea},
        page_table_wrapper::PageTableWrapped,
    },
    misc::stack_builder::StackBuilder,
};

pub mod clone;
pub mod mapping;
pub mod mem_area;

const USER_MEM_START: u64 = 0x30_0000_0000;
const KERNEL_MEM_START: u64 = 0xFFFF_8000_1000_0000;

static KERNEL_MEM: AtomicU64 = AtomicU64::new(KERNEL_MEM_START);

pub type AllocResult = (VirtAddr, StackBuilder);

#[derive(Debug)]
pub struct AddrSpace {
    pub memory_areas: Vec<MemoryArea>,
    pub page_table: PageTableWrapped,

    pub user_mem: VirtAddr,
}

impl Default for AddrSpace {
    fn default() -> Self {
        Self {
            memory_areas: Vec::default(),
            page_table: PageTableWrapped::default(),
            user_mem: VirtAddr::new(USER_MEM_START),
        }
    }
}

impl AddrSpace {
    pub fn load(&mut self) {
        self.page_table.load();
    }

    pub fn clean(&mut self) {
        log::debug!("addrspace: clean");
        // TODO: properly "clean" the memory lmao
        self.user_mem = VirtAddr::new(USER_MEM_START);
        self.page_table = PageTableWrapped::default();
        self.memory_areas = Vec::new();
    }

    pub fn translate_addr(&self, addr: VirtAddr) -> Option<PhysAddr> {
        self.page_table.inner.translate_addr(addr)
    }

    pub fn allocate_user(&mut self, pages: u64) -> AllocResult {
        log::trace!("addrspace: allocate_user pages {}", pages);
        let mem = self.user_mem;
        self.user_mem += (pages + 1) * 4096;

        self.map(MemoryArea::new(
            mem,
            pages,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
            Data::Normal,
            false,
        ))
    }

    pub fn allocate_user_lazy(&mut self, pages: u64) {
        log::trace!("addrspace: allocate_user_lazy pages {}", pages);
        let mem = self.user_mem;
        self.user_mem += (pages + 1) * 4096;

        self.map_lazy(MemoryArea::new(
            mem,
            pages,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
            Data::Normal,
            true,
        ));
    }

    pub fn allocate_kernel(&mut self, pages: u64) -> AllocResult {
        log::trace!("addrspace: allocate_kernel pages {}", pages);
        self.map(MemoryArea::new(
            VirtAddr::new(KERNEL_MEM.fetch_add((pages + 1) * 4096, Ordering::Relaxed)),
            pages,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            Data::Normal,
            false,
        ))
    }
}
