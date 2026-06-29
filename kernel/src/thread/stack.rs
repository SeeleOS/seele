use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, FrameDeallocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB,
    },
};

use crate::{
    memory::{
        paging::{FRAME_ALLOCATOR, MAPPER},
        utils::{Mut, apply_offset},
    },
    misc::stack_builder::StackBuilder,
};

static KERNEL_MEM: AtomicU64 = AtomicU64::new(0xFFFF_9000_3000_0000);

#[derive(Clone, Copy, Debug)]
struct StackSlot {
    start: Page<Size4KiB>,
    pages: u64,
}

lazy_static::lazy_static! {
    static ref FREE_STACK_SLOTS: Mut<Vec<StackSlot>> = Mut::new(Vec::new());
}

#[derive(Debug)]
pub struct KernelStack {
    start: Page<Size4KiB>,
    top: VirtAddr,
    frames: Vec<PhysFrame<Size4KiB>>,
}

impl KernelStack {
    pub fn top(&self) -> VirtAddr {
        self.top
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        if self.frames.is_empty() {
            return;
        }

        let mut mapper = MAPPER.get().unwrap().lock();
        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();

        for (index, frame) in self.frames.drain(..).enumerate() {
            let page = self.start + index as u64;
            if let Ok((unmapped_frame, flush)) = mapper.unmap(page) {
                flush.flush();
                debug_assert_eq!(unmapped_frame, frame);
            }

            unsafe {
                frame_allocator.deallocate_frame(frame);
            }
        }

        FREE_STACK_SLOTS.lock().push(StackSlot {
            start: self.start,
            pages: (self.top - self.start.start_address()) / 4096,
        });
    }
}

pub fn allocate_kernel_stack(pages: u64) -> StackBuilder {
    let guard_page = Page::containing_address(VirtAddr::new(
        KERNEL_MEM.fetch_add((pages + 1) * 4096, Ordering::Relaxed),
    ));
    let start = guard_page + 1;
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    let mut page_write_bases = Vec::with_capacity(pages as usize);
    let mut frame_allocator = FRAME_ALLOCATOR.try_get().unwrap().lock();

    for i in 0..pages {
        let page = start + i;
        let frame = frame_allocator.allocate_frame().expect("Memory full.");

        unsafe {
            MAPPER
                .get()
                .unwrap()
                .lock()
                .map_to(page, frame, flags, &mut *frame_allocator)
                .unwrap()
                .flush();
        };

        let write_addr = apply_offset(frame.start_address().as_u64());
        unsafe {
            let bytes = 4096;
            let start_ptr = write_addr as *mut u8;
            core::ptr::write_bytes(start_ptr, 0, bytes as usize);
        }

        page_write_bases.push(write_addr);
    }

    let end_addr = (start + pages).start_address();

    StackBuilder::new(end_addr.as_u64(), page_write_bases)
}

pub fn allocate_owned_kernel_stack(pages: u64) -> OwnedStackBuilder {
    let start = take_free_stack_slot(pages).unwrap_or_else(|| {
        let guard_page = Page::containing_address(VirtAddr::new(
            KERNEL_MEM.fetch_add((pages + 1) * 4096, Ordering::Relaxed),
        ));
        guard_page + 1
    });
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    let mut page_write_bases = Vec::with_capacity(pages as usize);
    let mut frames = Vec::with_capacity(pages as usize);
    let mut frame_allocator = FRAME_ALLOCATOR.try_get().unwrap().lock();

    for i in 0..pages {
        let page = start + i;
        let frame = frame_allocator.allocate_frame().expect("Memory full.");

        unsafe {
            MAPPER
                .get()
                .unwrap()
                .lock()
                .map_to(page, frame, flags, &mut *frame_allocator)
                .unwrap()
                .flush();
        };

        let write_addr = apply_offset(frame.start_address().as_u64());
        unsafe {
            let bytes = 4096;
            let start_ptr = write_addr as *mut u8;
            core::ptr::write_bytes(start_ptr, 0, bytes as usize);
        }

        page_write_bases.push(write_addr);
        frames.push(frame);
    }

    let end_addr = (start + pages).start_address();

    OwnedStackBuilder {
        builder: StackBuilder::new(end_addr.as_u64(), page_write_bases),
        stack: KernelStack {
            start,
            top: end_addr,
            frames,
        },
    }
}

fn take_free_stack_slot(pages: u64) -> Option<Page<Size4KiB>> {
    let mut slots = FREE_STACK_SLOTS.lock();
    let index = slots.iter().position(|slot| slot.pages == pages)?;
    Some(slots.swap_remove(index).start)
}

pub struct OwnedStackBuilder {
    builder: StackBuilder,
    stack: KernelStack,
}

impl OwnedStackBuilder {
    pub fn finish(self) -> KernelStack {
        let stack = self.stack;
        let _ = self.builder.finish();
        stack
    }
}
