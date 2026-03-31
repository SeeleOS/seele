use core::sync::atomic::Ordering;

use seele_sys::permission::Permissions;
use x86_64::{VirtAddr, structures::paging::PageTableFlags};

use crate::{
    memory::addrspace::mem_area::{Data, MemoryArea},
    misc::others::permissions_to_flags,
};

use super::{AddrSpace, AllocResult, KERNEL_MEM};

impl AddrSpace {
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

    pub fn allocate_user_lazy(
        &mut self,
        pages: u64,
        permissions: Permissions,
        data: Data,
    ) -> VirtAddr {
        log::trace!("addrspace: allocate_user_lazy pages {}", pages);
        let mem = self.fetch_add_user_mem(pages);
        let area = MemoryArea::new(mem, pages, permissions_to_flags(permissions), data, true);

        self.memory_areas.push(area.clone());

        area.start
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
