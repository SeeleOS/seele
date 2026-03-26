use core::sync::atomic::AtomicU64;

use alloc::vec::Vec;
use seele_sys::permission::Permissions;
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{FrameDeallocator, Mapper, Translate, page},
};

use crate::{
    memory::{
        addrspace::{
            cow::decrease_ref,
            mem_area::MemoryArea,
        },
        page_table_wrapper::PageTableWrapped,
        paging::{FRAME_ALLOCATOR, MAPPER},
    },
    misc::{others::permissions_to_flags, stack_builder::StackBuilder},
    s_print,
};

pub mod allocate;
pub mod clone;
pub mod cow;
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
        for area in &self.memory_areas {
            if !area.is_user() {
                continue;
            }

            for page in area.page_range() {
                if let Ok((frame, flush)) = self.page_table.inner.unmap(page) {
                    flush.flush();
                    if decrease_ref(frame) {
                        unsafe {
                            FRAME_ALLOCATOR
                                .get()
                                .unwrap()
                                .lock()
                                .deallocate_frame(frame);
                        }
                    }
                }
            }
        }
        s_print!("b");
        self.user_mem = VirtAddr::new(USER_MEM_START);
        self.page_table = PageTableWrapped::default();
        self.memory_areas = Vec::new();
        s_print!("ret");
    }

    pub fn update_permissions(&mut self, start: VirtAddr, end: VirtAddr, permissions: Permissions) {
        for area in &mut self.memory_areas {
            if area.start > start && area.end < end {
                area.flags = permissions_to_flags(permissions);

                for page in area.page_range() {
                    unsafe {
                        if let Ok(flush) = self
                            .page_table
                            .inner
                            .update_flags(page, permissions_to_flags(permissions))
                        {
                            flush.flush();
                        }
                    }
                }
            }
        }
    }

    pub fn translate_addr(&self, addr: VirtAddr) -> Option<PhysAddr> {
        self.page_table.inner.translate_addr(addr)
    }

    pub fn get_area(&self, addr: VirtAddr) -> Option<&MemoryArea> {
        self.memory_areas.iter().find(|p| p.contains(addr))
    }
}
