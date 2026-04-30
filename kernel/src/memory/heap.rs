use buddy_system_allocator::{Heap, LockedHeapWithRescue};
use core::{
    alloc::Layout,
    sync::atomic::{AtomicUsize, Ordering},
};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, PageTableFlags, Size4KiB,
    mapper::{MapToError, MapperFlushAll},
};

use crate::memory::{
    paging::{HEAP_BACKING_ALLOCATOR, MAPPER},
    utils::page_range_from_size,
};

#[global_allocator]
static HEAP_ALLOCATOR: LockedHeapWithRescue<32> = LockedHeapWithRescue::new(grow_heap);
static HEAP_MAPPED_BYTES: AtomicUsize = AtomicUsize::new(0);

// Memory area for the heap
pub const HEAP_START: usize = 0xFFFF_FFFF_4444_0000;
pub const HEAP_SIZE: usize = 256 * 1024 * 1024;
pub const INITIAL_HEAP_SIZE: usize = 64 * 1024 * 1024;
const HEAP_GROWTH_SIZE: usize = 8 * 1024 * 1024;
const HEAP_PAGE_TABLE_RESERVE_SIZE: usize = 2 * 1024 * 1024;
pub const HEAP_BACKING_RESERVE_SIZE: usize = HEAP_SIZE + HEAP_PAGE_TABLE_RESERVE_SIZE;

// Map the memory area for the heap from physical memory to virt memory
// and do some other stuff
// Note: cant call the map_area() function because
// MAPPER and FRAME_ALLOCATOR is not initalized
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    map_heap_range(HEAP_START, INITIAL_HEAP_SIZE, mapper, frame_allocator)?;
    unsafe {
        HEAP_ALLOCATOR.lock().init(HEAP_START, INITIAL_HEAP_SIZE);
    }
    HEAP_MAPPED_BYTES.store(INITIAL_HEAP_SIZE, Ordering::Release);

    Ok(())
}

fn grow_heap(heap: &mut Heap<32>, layout: &Layout) {
    let Ok(backing_allocator) = HEAP_BACKING_ALLOCATOR.try_get() else {
        return;
    };
    let Ok(mapper) = MAPPER.try_get() else {
        return;
    };
    let requested = layout
        .size()
        .max(layout.align())
        .next_power_of_two()
        .max(HEAP_GROWTH_SIZE);
    let grow_bytes =
        requested.min(HEAP_SIZE.saturating_sub(HEAP_MAPPED_BYTES.load(Ordering::Acquire)));
    if grow_bytes == 0 {
        return;
    }

    let mapped = HEAP_MAPPED_BYTES.load(Ordering::Acquire);
    let grow_start = HEAP_START + mapped;

    let mut mapper = mapper.lock();
    let mut backing_allocator = backing_allocator.lock();
    if map_heap_range(
        grow_start,
        grow_bytes,
        &mut *mapper,
        &mut *backing_allocator,
    )
    .is_err()
    {
        return;
    }

    unsafe {
        heap.add_to_heap(grow_start, grow_start + grow_bytes);
    }
    HEAP_MAPPED_BYTES.store(mapped + grow_bytes, Ordering::Release);
}

fn map_heap_range(
    start: usize,
    size: usize,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = page_range_from_size(start as u64, size as u64);
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;

        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.ignore();
        }
    }

    MapperFlushAll::new().flush_all();
    Ok(())
}
