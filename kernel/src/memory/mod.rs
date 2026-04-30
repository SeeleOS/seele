use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use limine::memory_map::{Entry, EntryType};
use spin::Mutex;

use crate::memory::{
    heap::{HEAP_BACKING_RESERVE_SIZE, init_heap},
    paging::{
        BootinfoFrameAllocator, BootstrapFrameAllocator, FRAME_ALLOCATOR, HEAP_BACKING_ALLOCATOR,
        MAPPER, init_mapper,
    },
};

pub mod addrspace;
pub mod heap;
pub mod mmio;
pub mod page_table_wrapper;
pub mod paging;
pub mod protection;
pub mod user_safe;
pub mod utils;

pub static PHYSICAL_MEMORY_OFFSET: OnceCell<u64> = OnceCell::uninit();
pub static USABLE_MEMORY_BYTES: OnceCell<u64> = OnceCell::uninit();
pub static MEMORY_REGIONS: OnceCell<&'static [&'static Entry]> = OnceCell::uninit();

pub fn init(physical_memory_offset: u64, memory_regions: &'static [&'static Entry]) {
    log::debug!("memory: init offset {:#x}", physical_memory_offset);
    let mut mapper = init_mapper(physical_memory_offset);
    let mut heap_backing_allocator = unsafe { BootstrapFrameAllocator::new(memory_regions) };
    {
        init_heap(&mut mapper, &mut heap_backing_allocator).expect("Failed heap initilization");
    }
    log::debug!("memory: heap ready");

    let runtime_cursor = {
        heap_backing_allocator
            .clone_after_reserving(HEAP_BACKING_RESERVE_SIZE.saturating_sub(heap::INITIAL_HEAP_SIZE))
            .expect("Failed to reserve heap backing frames")
    };
    let frame_allocator = BootinfoFrameAllocator::new(memory_regions, &runtime_cursor);

    let mapper = Arc::new(Mutex::new(mapper));
    let frame_allocator = Arc::new(Mutex::new(frame_allocator));

    MAPPER.get_or_init(|| mapper.clone());
    FRAME_ALLOCATOR.get_or_init(|| frame_allocator.clone());
    HEAP_BACKING_ALLOCATOR.get_or_init(|| Mutex::new(heap_backing_allocator));
    PHYSICAL_MEMORY_OFFSET.get_or_init(|| physical_memory_offset);
    MEMORY_REGIONS.get_or_init(|| memory_regions);
    USABLE_MEMORY_BYTES.get_or_init(|| {
        memory_regions
            .iter()
            .filter(|region| region.entry_type == EntryType::USABLE)
            .map(|region| region.length)
            .sum()
    });
    log::debug!("memory: mapper/frame allocator ready");
}

pub fn usable_memory_bytes() -> u64 {
    USABLE_MEMORY_BYTES.get().copied().unwrap_or(0)
}
