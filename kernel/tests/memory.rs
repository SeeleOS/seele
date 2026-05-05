#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod common;

use kernel::memory::paging::{FRAME_ALLOCATOR, MAPPER};
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags},
};

common::integration_test_entry!(test_main);

const TEST_MAPPING_ADDR: u64 = 0xffff_9000_0010_0000;

fn test_main() {
    let page = Page::containing_address(VirtAddr::new(TEST_MAPPING_ADDR));
    let frame = FRAME_ALLOCATOR
        .get()
        .expect("frame allocator missing")
        .lock()
        .allocate_frame()
        .expect("frame allocation failed");
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    unsafe {
        MAPPER
            .get()
            .expect("mapper missing")
            .lock()
            .map_to(
                page,
                frame,
                flags,
                &mut *FRAME_ALLOCATOR.get().unwrap().lock(),
            )
            .expect("map_to failed")
            .flush();
    }

    let ptr = page.start_address().as_mut_ptr::<u64>();
    unsafe {
        ptr.write_volatile(0x5152_5354_5556_5758);
        assert_eq!(ptr.read_volatile(), 0x5152_5354_5556_5758);
    }
}
