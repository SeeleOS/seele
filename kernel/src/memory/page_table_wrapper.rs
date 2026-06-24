use crate::memory::utils::MutGuard;
use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::{Cr3, Cr3Flags},
    structures::paging::{
        FrameAllocator, FrameDeallocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags,
        PhysFrame, Size4KiB, Translate, mapper::FlagUpdateError, mapper::MapToError,
        mapper::MapperFlush, mapper::TranslateError, mapper::UnmapError,
    },
};

use crate::memory::{
    PHYSICAL_MEMORY_OFFSET,
    paging::{BootinfoFrameAllocator, FRAME_ALLOCATOR},
    utils::{apply_offset, copy_kernel_mapping},
};

#[derive(Debug)]
pub struct PageTableWrapped {
    pub frame: PhysFrame<Size4KiB>,
}

impl Default for PageTableWrapped {
    fn default() -> Self {
        // allocates a frame for the l4 page table to be stored at
        let page_table_frame = FRAME_ALLOCATOR
            .get()
            .unwrap()
            .lock()
            .allocate_frame()
            .expect("No more space");

        let table_addr = VirtAddr::new(apply_offset(page_table_frame.start_address().as_u64()));

        // Get it as a page table
        let page_table: &mut PageTable = unsafe { &mut *(table_addr.as_mut_ptr()) };

        page_table.zero();

        copy_kernel_mapping(page_table);

        Self {
            frame: page_table_frame,
        }
    }
}

impl PageTableWrapped {
    pub fn new_with_frame_allocator(
        frame_allocator: &mut MutGuard<BootinfoFrameAllocator>,
    ) -> Self {
        let page_table_frame = frame_allocator.allocate_frame().expect("No more space");

        let table_addr = VirtAddr::new(apply_offset(page_table_frame.start_address().as_u64()));

        // Get it as a page table
        let page_table: &mut PageTable = unsafe { &mut *(table_addr.as_mut_ptr()) };

        page_table.zero();

        copy_kernel_mapping(page_table);

        Self {
            frame: page_table_frame,
        }
    }
}

impl PageTableWrapped {
    fn table_addr(&self) -> VirtAddr {
        VirtAddr::new(apply_offset(self.frame.start_address().as_u64()))
    }

    fn level_4_table_mut(&mut self) -> &mut PageTable {
        unsafe { &mut *self.table_addr().as_mut_ptr() }
    }

    fn table_from_frame(frame: PhysFrame<Size4KiB>) -> &'static mut PageTable {
        let table_addr = VirtAddr::new(apply_offset(frame.start_address().as_u64()));
        unsafe { &mut *table_addr.as_mut_ptr() }
    }

    fn collect_child_table_frames(
        table_frame: PhysFrame<Size4KiB>,
        level: usize,
        entry_range: core::ops::Range<usize>,
        frames: &mut alloc::vec::Vec<PhysFrame<Size4KiB>>,
    ) {
        if level == 1 {
            return;
        }

        let table = Self::table_from_frame(table_frame);
        for index in entry_range {
            let entry = &mut table[index];
            if entry.is_unused() || entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                continue;
            }

            let Ok(child_frame) = entry.frame() else {
                continue;
            };
            Self::collect_child_table_frames(child_frame, level - 1, 0..512, frames);
            entry.set_unused();
            frames.push(child_frame);
        }
    }

    pub fn deallocate_user_page_tables(self) {
        let mut frames = alloc::vec::Vec::new();
        Self::collect_child_table_frames(self.frame, 4, 0..128, &mut frames);
        frames.push(self.frame);

        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
        for frame in frames {
            unsafe {
                frame_allocator.deallocate_frame(frame);
            }
        }
    }

    fn with_mapper<R>(&mut self, f: impl FnOnce(&mut OffsetPageTable<'_>) -> R) -> R {
        let page_table = self.level_4_table_mut();
        let phys_offset = VirtAddr::new(*PHYSICAL_MEMORY_OFFSET.get().unwrap());
        let mut mapper = unsafe { OffsetPageTable::new(page_table, phys_offset) };
        f(&mut mapper)
    }

    pub fn translate(
        &mut self,
        addr: VirtAddr,
    ) -> x86_64::structures::paging::mapper::TranslateResult {
        self.with_mapper(|mapper| mapper.translate(addr))
    }

    pub fn translate_addr(&mut self, addr: VirtAddr) -> Option<PhysAddr> {
        self.with_mapper(|mapper| mapper.translate_addr(addr))
    }

    pub fn translate_page(
        &mut self,
        page: Page<Size4KiB>,
    ) -> Result<PhysFrame<Size4KiB>, TranslateError> {
        self.with_mapper(|mapper| mapper.translate_page(page))
    }

    /// # Safety
    ///
    /// The caller must ensure that `page`, `frame`, and `flags` describe a valid mapping
    /// operation for this page table and that `allocator` can safely provide any required
    /// intermediate page-table frames.
    pub unsafe fn map_to<A: FrameAllocator<Size4KiB> + ?Sized>(
        &mut self,
        page: Page<Size4KiB>,
        frame: PhysFrame<Size4KiB>,
        flags: PageTableFlags,
        allocator: &mut A,
    ) -> Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>> {
        self.with_mapper(|mapper| unsafe { mapper.map_to(page, frame, flags, allocator) })
    }

    pub fn unmap(
        &mut self,
        page: Page<Size4KiB>,
    ) -> Result<(PhysFrame<Size4KiB>, MapperFlush<Size4KiB>), UnmapError> {
        self.with_mapper(|mapper| mapper.unmap(page))
    }

    /// # Safety
    ///
    /// The caller must ensure that updating `page` with `flags` preserves all required page-table
    /// invariants for this address space.
    pub unsafe fn update_flags(
        &mut self,
        page: Page<Size4KiB>,
        flags: PageTableFlags,
    ) -> Result<MapperFlush<Size4KiB>, FlagUpdateError> {
        self.with_mapper(|mapper| unsafe { mapper.update_flags(page, flags) })
    }

    pub fn load(&mut self) {
        unsafe {
            Cr3::write(self.frame, Cr3Flags::empty());
        }
    }

    pub fn is_loaded(&self) -> bool {
        Cr3::read().0 == self.frame
    }
}
