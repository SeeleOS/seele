use core::sync::atomic::AtomicU64;

use alloc::vec::Vec;
use seele_sys::permission::Permissions;
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{FrameDeallocator, Mapper, Page, Size4KiB, Translate, page},
};

use crate::{
    memory::{
        addrspace::{
            cow::decrease_ref,
            mem_area::{Data, MemoryArea},
            misc::split_memory_area,
        },
        page_table_wrapper::PageTableWrapped,
        paging::{FRAME_ALLOCATOR, MAPPER},
    },
    misc::{others::permissions_to_flags, stack_builder::StackBuilder},
    s_print,
};

pub mod allocate;
pub mod apply;
pub mod clone;
pub mod cow;
pub mod mapping;
pub mod mem_area;
pub mod misc;

pub const LAZY_MAP: bool = true;

const USER_MEM_START: u64 = 0x30_0000_0000;
pub const KERNEL_MEM_START: u64 = 0xFFFF_8000_1000_0000;

static KERNEL_MEM: AtomicU64 = AtomicU64::new(KERNEL_MEM_START);

pub type AllocResult = (VirtAddr, StackBuilder);

#[derive(Debug)]
pub struct AddrSpace {
    pub memory_areas: Vec<MemoryArea>,
    pub page_table: PageTableWrapped,

    pub user_mem: VirtAddr,
    pub last_area_index: Option<usize>,
}

impl Default for AddrSpace {
    fn default() -> Self {
        Self {
            memory_areas: Vec::default(),
            page_table: PageTableWrapped::default(),
            user_mem: VirtAddr::new(USER_MEM_START),
            last_area_index: None,
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
        self.user_mem = VirtAddr::new(USER_MEM_START);
        self.page_table = PageTableWrapped::default();
        self.memory_areas = Vec::new();
        self.last_area_index = None;
    }

    pub fn update_permissions(&mut self, start: VirtAddr, end: VirtAddr, permissions: Permissions) {
        if start >= end {
            return;
        }

        let new_flags = permissions_to_flags(permissions);
        let mut new_areas = Vec::new();

        for area in self.memory_areas.drain(..) {
            let overlap_start = core::cmp::max(area.start, start);
            let overlap_end = core::cmp::min(area.end, end);

            if overlap_start >= overlap_end {
                new_areas.push(area);
                continue;
            }

            let (left, right) = split_memory_area(&area, start, end);

            if let Some(left) = left {
                new_areas.push(left);
            }

            let mut middle = area.clone();
            middle.start = overlap_start;
            middle.end = overlap_end;
            middle.flags = new_flags;

            if let Data::File {
                offset,
                file_bytes,
                file,
            } = &area.data
            {
                middle.data = Data::File {
                    offset: *offset + (overlap_start.as_u64() - area.start.as_u64()),
                    file_bytes: file_bytes
                        .saturating_sub(overlap_start.as_u64() - area.start.as_u64()),
                    file: file.clone(),
                };
            }

            new_areas.push(middle);

            if let Some(right) = right {
                new_areas.push(right);
            }
        }

        self.memory_areas = new_areas;
        self.last_area_index = None;

        let last_addr = end - 1u64;
        for page in Page::<Size4KiB>::range_inclusive(
            Page::<Size4KiB>::containing_address(start),
            Page::<Size4KiB>::containing_address(last_addr),
        ) {
            unsafe {
                if let Ok(flush) = self.page_table.inner.update_flags(page, new_flags) {
                    flush.flush();
                }
            }
        }
    }

    pub fn translate_addr(&self, addr: VirtAddr) -> Option<PhysAddr> {
        self.page_table.inner.translate_addr(addr)
    }

    pub fn get_area(&mut self, addr: VirtAddr) -> Option<&MemoryArea> {
        if let Some(index) = self.last_area_index
            && self
                .memory_areas
                .get(index)
                .is_some_and(|area| area.contains(addr))
        {
            return self.memory_areas.get(index);
        }

        let mut left = 0usize;
        let mut right = self.memory_areas.len();

        while left < right {
            let mid = left + (right - left) / 2;
            let area = &self.memory_areas[mid];

            if addr < area.start {
                right = mid;
            } else if addr >= area.end {
                left = mid + 1;
            } else {
                self.last_area_index = Some(mid);
                return self.memory_areas.get(mid);
            }
        }

        self.last_area_index = None;
        None
    }
}
