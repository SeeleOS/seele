use core::intrinsics::copy_nonoverlapping;

use alloc::collections::btree_map::BTreeMap;
use ext4plus::sync::Mutex;
use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, FrameDeallocator, Mapper, Page, PageTableFlags, PhysFrame, Translate,
        mapper::TranslateResult,
    },
};

use crate::memory::{addrspace::AddrSpace, paging::FRAME_ALLOCATOR, utils::apply_offset};

lazy_static::lazy_static! {
    static ref FRAME_REF_COUNT: Mutex<BTreeMap<u64, usize>> = Mutex::new(BTreeMap::new());
}

pub const COW_FLAG: PageTableFlags = PageTableFlags::BIT_9;
impl AddrSpace {
    // Replace the readonly CoW page with a normal page.
    pub fn replace_cow_page(&mut self, addr: VirtAddr) {
        let page = Page::containing_address(addr);

        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();

        let new_frame = frame_allocator.allocate_frame().unwrap();
        let new_addr = apply_offset(new_frame.start_address().as_u64());

        let TranslateResult::Mapped { mut flags, .. } =
            self.page_table.inner.translate(page.start_address())
        else {
            return;
        };

        let (old_frame, flush) = self.page_table.inner.unmap(page).unwrap();
        flush.flush();

        flags.remove(COW_FLAG);
        flags |= PageTableFlags::WRITABLE;

        unsafe {
            copy_nonoverlapping(
                apply_offset(old_frame.start_address().as_u64()) as *const u8,
                new_addr as *mut u8,
                4096,
            );

            self.page_table
                .inner
                .map_to(page, new_frame, flags, &mut *frame_allocator)
                .unwrap()
                .flush()
        };

        increase_ref(new_frame);
        decrease_ref(old_frame);
    }
}

pub fn increase_ref(frame: PhysFrame) {
    *FRAME_REF_COUNT
        .lock()
        .entry(frame.start_address().as_u64())
        .or_insert(0) += 1;
}

pub fn decrease_ref(frame: PhysFrame) {
    let mut ref_counter_locked = FRAME_REF_COUNT.lock();
    if let Some(count) = ref_counter_locked.get_mut(&frame.start_address().as_u64()) {
        *count -= 1;

        if *count == 0 {
            ref_counter_locked.remove(&frame.start_address().as_u64());
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
