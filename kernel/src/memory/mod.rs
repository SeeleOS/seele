use alloc::sync::Arc;
use bootloader_api::info::MemoryRegions;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::memory::{
    heap::init_heap,
    paging::{BootinfoFrameAllocator, FRAME_ALLOCATOR, MAPPER, init_mapper},
};

pub mod addrspace;
pub mod fixed_block_size;
pub mod heap;
pub mod mmio;
pub mod page_table_wrapper;
pub mod paging;
pub mod utils;

pub static PHYSICAL_MEMORY_OFFSET: OnceCell<u64> = OnceCell::uninit();

pub fn init(physical_memory_offset: u64, memory_regions: &'static MemoryRegions) {
    log::debug!("memory: init offset {:#x}", physical_memory_offset);
    let mut mapper = init_mapper(physical_memory_offset);
    let mut frame_allocator = unsafe { BootinfoFrameAllocator::new(memory_regions) };
    init_heap(&mut mapper, &mut frame_allocator).expect("Failed heap initilization");
    log::debug!("memory: heap ready");

    // [TODO] maybe i should move some stuff out of the os struct? tho if it works, dont touch it
    let mapper = Arc::new(Mutex::new(mapper));
    let frame_allocator = Arc::new(Mutex::new(frame_allocator));

    // inits mapper and frame allocator
    MAPPER.get_or_init(|| mapper.clone());
    FRAME_ALLOCATOR.get_or_init(|| frame_allocator.clone());
    PHYSICAL_MEMORY_OFFSET.get_or_init(|| physical_memory_offset);
    log::debug!("memory: mapper/frame allocator ready");
}
