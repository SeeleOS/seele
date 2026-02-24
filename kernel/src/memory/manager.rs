use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB},
};

use crate::{
    graphics::framebuffer::FRAME_BUFFER,
    memory::{paging::FRAME_ALLOCATOR, utils::apply_offset},
    utils::stack_builder::StackBuilder,
};

static USER_MEM: AtomicU64 = AtomicU64::new(0x30_0000_0000);
static KERNEL_MEM: AtomicU64 = AtomicU64::new(0xFFFF_8000_1000_0000);

/// Returns the virt addr of the mem start and mem end
///
/// Note: The phys addr of the stack top is the addr of the
/// last frame, so if you writes more then 4KiB of memory
/// it will cause undefined behaviour
pub fn allocate_user_mem(
    pages: u64,
    table: &mut OffsetPageTable<'static>,
    flags: PageTableFlags,
) -> (VirtAddr, StackBuilder) {
    // skips the guard page
    let guard_page = allocate_user_page(pages);
    let start = guard_page + 1;

    let mut last_frame = None;
    let mut frame_allocator = FRAME_ALLOCATOR.try_get().unwrap().lock();

    let mut flags = flags;

    if !flags.contains(PageTableFlags::USER_ACCESSIBLE) {
        flags |= PageTableFlags::USER_ACCESSIBLE;
    }

    for i in 0..pages {
        let page = start + i;
        let frame = frame_allocator.allocate_frame().expect("Memory full.");

        unsafe {
            table
                .map_to(page, frame, flags, &mut *frame_allocator)
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

pub fn allocate_kernel_mem(
    pages: u64,
    table: &mut OffsetPageTable<'static>,
    flags: PageTableFlags,
) -> (VirtAddr, StackBuilder) {
    let guard_page = allocate_kernel_page(pages);
    let start = guard_page + 1;

    let mut frame_allocator = FRAME_ALLOCATOR.try_get().unwrap().lock();
    let mut last_frame = None;

    for i in 0..pages {
        let page = start + i;
        let frame = frame_allocator.allocate_frame().expect("Memory full.");

        unsafe {
            table
                .map_to(page, frame, flags, &mut *frame_allocator)
                .unwrap()
                .flush();
        };

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

fn allocate_user_page(count: u64) -> Page<Size4KiB> {
    Page::containing_address(VirtAddr::new(
        USER_MEM.fetch_add((count + 1) * 4096, Ordering::Relaxed),
    ))
}
fn allocate_kernel_page(count: u64) -> Page<Size4KiB> {
    Page::containing_address(VirtAddr::new(
        KERNEL_MEM.fetch_add((count + 1) * 4096, Ordering::Relaxed),
    ))
}
