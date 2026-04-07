use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
        mapper::{MapToError, MapperFlushAll},
    },
};

use crate::memory::{fixed_block_size::FixedBlockSizeAllocator, utils::Locked};

#[global_allocator]
static HEAP_ALLOCATOR: Locked<FixedBlockSizeAllocator> =
    Locked::new(FixedBlockSizeAllocator::new());

// Memory area for the heap
pub const HEAP_START: usize = 0xFFFF_FFFF_4444_0000;
pub const HEAP_SIZE: usize = 20 * 1024 * 1024;

// Map the memory area for the heap from physical memory to virt memory
// and do some other stuff
// Note: cant call the map_area() function because
// MAPPER and FRAME_ALLOCATOR is not initalized
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    // Page range of the heap
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.ignore();
        }
    }

    MapperFlushAll::new().flush_all();

    // initalize the heap allocator with the heap memory
    unsafe {
        HEAP_ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}
