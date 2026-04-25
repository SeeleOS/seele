use core::sync::atomic::Ordering;

use x86_64::{VirtAddr, structures::paging::PageTableFlags};

use crate::{
    memory::{
        addrspace::mem_area::{Data, MemoryArea},
        protection::Protection,
    },
    misc::others::protection_to_page_flags,
};

use super::{AddrSpace, AllocResult, KERNEL_MEM};

impl AddrSpace {
    pub fn allocate_user(&mut self, pages: u64) -> AllocResult {
        log::trace!("addrspace: allocate_user pages {}", pages);
        let mem = self.user_mem;
        self.user_mem += (pages + 1) * 4096;

        self.map(MemoryArea::new_with_guard(
            mem,
            pages,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
            Data::Normal,
            false,
        ))
    }

    pub fn allocate_user_stack(&mut self, pages: u64) -> AllocResult {
        log::trace!("addrspace: allocate_user pages {}", pages);
        let (start, mut stack_builder) = self.allocate_user(pages);
        // Leave one page free above the initial userspace stack contents.
        stack_builder.reserve_headroom(4096);
        (start, stack_builder)
    }

    pub fn allocate_user_lazy(
        &mut self,
        pages: u64,
        protection: Protection,
        data: Data,
    ) -> VirtAddr {
        log::trace!("addrspace: allocate_user_lazy pages {}", pages);
        let mem = self.fetch_add_user_mem(pages);
        let area = MemoryArea::new(mem, pages, protection_to_page_flags(protection), data, true);

        self.register_area(area);

        mem
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
